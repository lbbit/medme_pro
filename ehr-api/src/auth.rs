// medme-ehr · T4 鉴权：静态 Bearer token（env 注入）+ 只读/读写分权。
//
// 设计约束（与本项目既有不变量同形）：
// ① **零新依赖**：token 表用手写解析 + 手写常量时间比较（不引 subtle/argon2 等）。
//    理由同 D-017：鉴权协议面小到能被测试钉死；且「静态 token」本身不提供
//    「抗暴力破解」能力，真正的防线是「不放在公网裸奔 + 定期轮换」。
// ② **诚实分权**：token 带 scope（ro / rw）。ro 只能读；写操作(POST/PUT/PATCH/DELETE)
//    遇 ro token ⇒ 403（不是 401：token 有效，只是权限不足）。D5：不把两类问题混成一个码。
// ③ **可关闭**：未配置任何 token ⇒ 服务以「open 模式」运行（保持向后兼容，
//    本地开发/既有门禁脚本不必改），但启动 [证据] 行必须显式打出 auth=open，
//    杜绝「以为开了鉴权其实没开」。
// ④ **env 是唯一配置面**：`EHR_API_TOKENS`（多 token，逗号分隔）与 `EHR_API_TOKEN`
//    （单 token，等价 rw，兼容 skill helper 已有变量）⇒ 升级/换机器不改文件（D4 纪律）。
//
// token 语法（`EHR_API_TOKENS`）:
//   <token>[:<scope>]           —— scope ∈ {ro, rw}，缺省 rw
//   <name>=<token>[:<scope>]    —— 可选名字，仅用于日志/排障
// 例：EHR_API_TOKENS="abc123:rw,def456:ro,agent=xyz:ro"
// 允许 token 内含 '-' '_' 十六进制字符；':' 与 '=' 需转义约定：不支持。
use std::collections::HashMap;

/// token 权限级别
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// 只读：GET/HEAD 放行，写操作 403
    Ro,
    /// 读写：放行全部已实装端点
    Rw,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Ro => "ro",
            Scope::Rw => "rw",
        }
    }
    /// 该 scope 是否允许某个 HTTP 方法
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "ro" => Ok(Scope::Ro),
            "rw" => Ok(Scope::Rw),
            other => Err(format!("unknown scope {other:?} (expected ro|rw)")),
        }
    }
    pub fn allows(self, method: &str) -> bool {
        match self {
            Scope::Rw => true,
            Scope::Ro => matches!(method, "GET" | "HEAD" | "OPTIONS"),
        }
    }
    /// 该 scope 是否允许某个 MCP 工具（写侧工具需 rw；读侧工具 ro 即可）
    pub fn allows_tool(self, tool: &str) -> bool {
        match self {
            Scope::Rw => true,
            // 写侧工具名单**只有一处**（mcp::WRITE_TOOLS）⇒ 工具分权与工具公布不会漂移。
            Scope::Ro => !crate::mcp::WRITE_TOOLS.contains(&tool),
        }
    }
}

/// token 表（不可变）。map: token → (name, scope)
#[derive(Clone, Debug, Default)]
pub struct TokenSet {
    map: HashMap<String, (String, Scope)>,
}

impl TokenSet {
    /// 解析 token 表文本；格式错误以 `Err` 返回并带具体原因（D5：参数校验必须带原因）。
    /// 同一 token 重复出现 ⇒ Err（静默取后者会让「轮换」这类意图失真）。
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut map = HashMap::new();
        for raw in spec.split(',') {
            let entry = raw.trim();
            if entry.is_empty() {
                continue;
            }
            // 先切可选 name=，再切可选 :scope
            let (name, rest) = match entry.split_once('=') {
                Some((n, r)) => (n.trim().to_string(), r.trim()),
                None => (String::new(), entry),
            };
            let (token, scope_str) = match rest.rsplit_once(':') {
                Some((t, s)) => (t.trim(), s.trim()),
                None => (rest, "rw"),
            };
            if token.is_empty() {
                return Err(format!("empty token in spec entry: {entry:?}"));
            }
            let scope = match scope_str {
                "ro" => Scope::Ro,
                "rw" => Scope::Rw,
                other => {
                    return Err(format!(
                        "unknown scope {other:?} in entry {entry:?} (expected ro|rw)"
                    ))
                }
            };
            if map.contains_key(token) {
                return Err(format!("duplicate token in spec (entry {entry:?})"));
            }
            let name = if name.is_empty() {
                format!("tok{}", map.len() + 1)
            } else {
                name
            };
            map.insert(token.to_string(), (name, scope));
        }
        Ok(TokenSet { map })
    }

    /// 从环境读取：EHR_API_TOKENS（多）+ EHR_API_TOKEN（单，rw，兼容既有 skill 变量）。
    /// 两处都为空 ⇒ 空表（open 模式）。
    pub fn from_env() -> Result<Self, String> {
        let mut set = Self::default();
        if let Ok(spec) = std::env::var("EHR_API_TOKENS") {
            if !spec.trim().is_empty() {
                set = Self::parse(&spec)?;
            }
        }
        if let Ok(tok) = std::env::var("EHR_API_TOKEN") {
            let tok = tok.trim();
            if !tok.is_empty() {
                if set.map.contains_key(tok) {
                    // 同一 token 出现在两个变量里：不是错误，但重复登记无意义 ⇒ 忽略后者
                } else {
                    set.map
                        .insert(tok.to_string(), ("EHR_API_TOKEN".to_string(), Scope::Rw));
                }
            }
        }
        Ok(set)
    }

    /// 是否为 open 模式（未配置任何 token）
    pub fn is_open(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 校验 Bearer 头；返回该 token 的 scope。
    /// token 比较用常量时间（避免逐字节早退泄漏前缀）——手写实现，同 D-019 base64 的严格性思路。
    pub fn authenticate(&self, bearer: &str) -> Option<(String, Scope)> {
        let token = bearer.strip_prefix("Bearer ").or_else(|| bearer.strip_prefix("bearer "))?;
        let token = token.trim();
        if token.is_empty() {
            return None;
        }
        let mut hit: Option<(&String, &(String, Scope))> = None;
        for (k, v) in &self.map {
            if const_eq(k.as_bytes(), token.as_bytes()) {
                hit = Some((k, v));
            }
        }
        hit.map(|(_, v)| (v.0.clone(), v.1))
    }
}

/// 常量时间字节比较：长度不等也走完整轮次（避免长度泄漏），逐字节累积差异。
fn const_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scope_and_name() {
        let s = TokenSet::parse("a:ro, b=bb:rw, c").unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(s.authenticate("Bearer a").unwrap().1, Scope::Ro);
        assert_eq!(s.authenticate("Bearer bb").unwrap().0, "b");
        assert_eq!(s.authenticate("Bearer bb").unwrap().1, Scope::Rw);
        assert_eq!(s.authenticate("Bearer c").unwrap().1, Scope::Rw); // 缺省 rw
    }

    #[test]
    fn parse_rejects_bad_input_with_reason() {
        assert!(TokenSet::parse("a:xx").unwrap_err().contains("unknown scope"));
        assert!(TokenSet::parse(":ro").unwrap_err().contains("empty token"));
        assert!(TokenSet::parse("a:ro,a:rw").unwrap_err().contains("duplicate"));
    }

    #[test]
    fn scope_permission_matrix() {
        assert!(Scope::Ro.allows("GET"));
        assert!(!Scope::Ro.allows("POST"));
        assert!(!Scope::Ro.allows("DELETE"));
        assert!(Scope::Rw.allows("POST"));
        assert!(!Scope::Ro.allows_tool("create_person"));
        assert!(Scope::Ro.allows_tool("list_persons"));
        assert!(Scope::Rw.allows_tool("upload_document"));
    }

    #[test]
    fn authenticate_rejects_wrong_and_missing_bearer() {
        let s = TokenSet::parse("secret:ro").unwrap();
        assert!(s.authenticate("Bearer wrong").is_none());
        assert!(s.authenticate("secret").is_none()); // 缺 Bearer 前缀
        assert!(s.authenticate("Bearer ").is_none());
        assert!(s.authenticate("Bearer secret").is_some());
    }

    #[test]
    fn const_eq_matches_std_eq() {
        assert!(const_eq(b"abc", b"abc"));
        assert!(!const_eq(b"abc", b"abd"));
        assert!(!const_eq(b"abc", b"abcd"));
        assert!(const_eq(b"", b""));
    }
}

// ---- T46：token 持久化管理面（Web 端生成/查询）----
//
// 设计（D-064）：
// ① **文件是真相**：`{EHR_HOME}/registry/tokens.jsonl`（append-only，与 persons.jsonl 同哲学——
//    整行一次写 + 进程内互斥）。每行 {"op":"add","name":..,"token":..,"scope":"ro|rw","at":..}
//    或 {"op":"revoke","name":..,"at":..}（revoke 按 name 判，token 本身不再落日志）。
// ② **启动期合成**：env `EHR_API_TOKENS`（启动种子，兼容既有部署）+ 文件回放 ⇒ 运行期 TokenSet。
//    文件里 add 与 env 重复 token ⇒ 文件为准（env 是种子不是上限）。
// ③ **运行期可变**：Web 端「生成 token」= append 一行 + 内存表即时生效；「查询」= 列出 name/scope/at
//    （**token 明文只在创建回执里返回一次**，列表只给前 6 位指纹——防页面泄露）。
//    revoke 按 name ⇒ 内存表移除 + 文件留痕。
// ④ **读写分权**：管理面本身要求 rw scope（ro token 不能管理 token——防只读 token 提权）。
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TokenListEntry {
    pub name: String,
    pub scope: Scope,
    pub token_prefix: String, // 前 10 位（指纹），明文不回传
    pub source: &'static str, // "env" | "file"
    #[serde(rename = "at")]
    pub at: String,
}

pub struct TokenManager {
    file: PathBuf, // {home}/registry/tokens.jsonl
    inner: Mutex<HashMap<String, (String, Scope)>>, // token → (name, scope) 运行期权威表
}

impl TokenManager {
    /// env 种子 + 文件回放 → 运行期表。文件坏行（非末行）按 D-050 红脸。
    pub fn load(home: &std::path::Path, env_set: TokenSet) -> Result<Self, String> {
        let file = home.join("registry").join("tokens.jsonl");
        let mut map = env_set.map.clone();
        if let Ok(content) = std::fs::read_to_string(&file) {
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let last = i == lines.len() - 1;
                let ev: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                    if last && !content.ends_with('\n') {
                        format!("tokens.jsonl 末行在途（并发写在途，跳过）: {e}")
                    } else {
                        format!("tokens.jsonl 完整行解析失败（真相层坏，必须红）: {e}")
                    }
                })?;
                match ev.get("op").and_then(|o| o.as_str()) {
                    Some("add") => {
                        let (token, name, scope) = (
                            ev.get("token").and_then(|t| t.as_str()).unwrap_or(""),
                            ev.get("name").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                            ev.get("scope").and_then(|t| t.as_str()).unwrap_or("rw"),
                        );
                        if token.is_empty() {
                            return Err(format!("tokens.jsonl 行 {} 空 token", i + 1));
                        }
                        let scope = Scope::parse(scope).map_err(|e| format!("行 {} scope: {e}", i + 1))?;
                        map.insert(token.to_string(), (name, scope));
                    }
                    Some("revoke") => {
                        if let Some(name) = ev.get("name").and_then(|t| t.as_str()) {
                            map.retain(|_, (n, _)| n != name);
                        }
                    }
                    other => return Err(format!("tokens.jsonl 行 {} 未知 op: {other:?}", i + 1)),
                }
            }
        }
        Ok(Self { file, inner: Mutex::new(map) })
    }

    fn append_line(&self, ev: &serde_json::Value) -> Result<(), String> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut line = serde_json::to_vec(ev).map_err(|e| e.to_string())?;
        line.push(b'\n');
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.file)
            .map_err(|e| e.to_string())?;
        f.write_all(&line).map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())
    }

    /// 空表构造（tests/dry-run）。open 模式判定与 TokenSet.is_open 同语义。
    pub fn empty() -> Self { Self { file: PathBuf::new(), inner: Mutex::new(HashMap::new()) } }
    /// open 模式下的持久化 manager：file 指向真实路径（首条 token 落盘 + 即时关闸）。
    /// empty() 的 file 为空路径 ⇒ create 会失败 ⇒ open 模式必须用这个构造。
    pub fn open_mode(home: &std::path::Path) -> Self {
        Self { file: home.join("registry").join("tokens.jsonl"), inner: Mutex::new(HashMap::new()) }
    }
    pub fn is_open(&self) -> bool { self.inner.lock().unwrap_or_else(|p| p.into_inner()).is_empty() }

    /// 生成新 token：随机 32 字节 hex（前缀 ehr-），scope 由调用方给。
    /// 返回明文（**只在本次回执出现**）；表即时生效。
    pub fn create(&self, name: &str, scope: Scope) -> Result<(String, TokenListEntry), String> {
        let b: [u8; 24] = rand::random();
        let token = format!("ehr-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>());
        {
            let m = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            if m.values().any(|(n, _)| n == name) {
                return Err(format!("token name {name:?} already exists"));
            }
        }
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.append_line(&serde_json::json!({"op":"add","name":name,"token":token,"scope":scope.as_str(),"at":at}))?;
        let entry = TokenListEntry {
            name: name.to_string(),
            scope,
            token_prefix: token.chars().take(10).collect(),
            source: "file",
            at,
        };
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).insert(token.clone(), (name.to_string(), scope));
        Ok((token, entry))
    }

    pub fn list(&self) -> Vec<TokenListEntry> {
        let m = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut out: Vec<TokenListEntry> = m.iter().map(|(t, (n, s))| TokenListEntry {
            name: n.clone(), scope: *s,
            token_prefix: t.chars().take(10).collect(),
            source: "env", at: String::new(),
        }).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn revoke(&self, name: &str) -> Result<bool, String> {
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let existed = {
            let mut m = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            let before = m.len();
            m.retain(|_, (n, _)| n != name);
            before != m.len()
        };
        if existed {
            self.append_line(&serde_json::json!({"op":"revoke","name":name,"at":at}))?;
        }
        Ok(existed)
    }

    /// 与 TokenSet::authenticate 同签名/同语义（供中间件）：入参=完整 Authorization 值。
    /// 运行期表可变 ⇒ 认证走 manager（Web 生成的 token 即时生效，无需重启）。
    pub fn authenticate(&self, bearer: &str) -> Option<(String, Scope)> {
        let token = bearer.strip_prefix("Bearer ").or_else(|| bearer.strip_prefix("bearer "))?;
        let token = token.trim();
        if token.is_empty() {
            return None;
        }
        let m = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        m.get(token).cloned()
    }
}
