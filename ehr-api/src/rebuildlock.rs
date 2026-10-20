// 重建期互斥锁（D-024）：**重建是离线动作**，重建期间必须禁止对派生层的并发写。
//
// 背景（D-023 未销账项①）：`--rebuild-index` 会删掉 {home}/vaults/{pid}/medme.db 后从事件日志回放。
// 若此时服务仍在运行并写同一箱，会出现「重建把刚写进去的派生状态抹掉」或「半写状态被回放覆盖」。
// 之前只有「建议停服重建」的口头约定，**没有机制**——本模块把约定变成机制。
//
// 机制选型：**锁文件 + 原子创建**（O_CREAT|O_EXCL 语义，POSIX 原子）。
//   - 不用 flock：libc 不是直接依赖，且 flock 语义要跨容器/跨 NFS 反复验证；
//   - 不用 SQLite 锁：重建要删的正是 SQLite 文件，锁不能住在被删对象里；
//   - 锁文件放 {home}/.rebuild.lock（**不在 vaults/ 下** ⇒ 重建不会碰它；registry 之外，不污染真相层语义）。
//
// 崩溃残留（stale）处理：锁文件里写 pid + 时间戳；取锁时若发现锁存在但持锁进程已不在（/proc/<pid> 不存在）
//   ⇒ 判定为陈旧锁，回收它（否则一次 kill -9 就把服务永久锁死，这是不可接受的运维陷阱）。
//   进程存活检查用 /proc（Linux）——本项目部署目标就是 Linux 容器。
use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 锁文件名（放在 home 根，不在 vaults/ 下）
pub const LOCK_FILE_NAME: &str = ".rebuild.lock";

/// 锁文件路径
pub fn lock_path(home: &Path) -> PathBuf {
    home.join(LOCK_FILE_NAME)
}

/// 持锁者信息（写入锁文件的内容）
#[derive(Debug, Clone)]
pub struct LockInfo {
    pub pid: u32,
    pub ts: String,
    pub mode: String, // "rebuild" / "test"
}

impl LockInfo {
    fn to_line(&self) -> String {
        format!("pid={}\nts={}\nmode={}\n", self.pid, self.ts, self.mode)
    }
    fn parse(s: &str) -> Option<LockInfo> {
        let mut pid = None;
        let mut ts = String::new();
        let mut mode = String::new();
        for line in s.lines() {
            if let Some(v) = line.strip_prefix("pid=") {
                pid = v.trim().parse::<u32>().ok();
            } else if let Some(v) = line.strip_prefix("ts=") {
                ts = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("mode=") {
                mode = v.trim().to_string();
            }
        }
        Some(LockInfo { pid: pid?, ts, mode })
    }
}

/// 进程是否还活着（Linux /proc）。判不出就当活着（保守：宁可误阻塞，也不要并发写坏派生层）
fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// 当前是否处于「重建锁」状态（服务面用来挡写）。
/// 返回 Some(info) = 有人在重建；None = 没人重建（或锁是陈旧的且已被忽略）。
pub fn inspect(home: &Path) -> Option<LockInfo> {
    let p = lock_path(home);
    let content = std::fs::read_to_string(&p).ok()?;
    let info = LockInfo::parse(&content)?;
    if pid_alive(info.pid) {
        Some(info)
    } else {
        // 陈旧锁：持锁进程已死。**不自动删除**（删除是取锁方的动作），但对服务面而言它不构成阻塞。
        None
    }
}

/// 重建互斥锁的 RAII 守卫：存在即持有，drop 即释放（文件删除）。
#[derive(Debug)]
pub struct RebuildLock {
    path: PathBuf,
}

impl RebuildLock {
    /// 尝试取锁。已被他人持锁 ⇒ 报错（不等待：重建是运维动作，冲突应显式暴露而不是默默排队）。
    pub fn acquire(home: &Path, mode: &str) -> Result<RebuildLock> {
        let path = lock_path(home);
        // 陈旧锁回收：锁存在但持锁进程已死 ⇒ 删掉重取（否则一次 kill -9 永久锁死）
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(info) = LockInfo::parse(&content) {
                if !pid_alive(info.pid) {
                    let _ = std::fs::remove_file(&path); // 陈旧锁回收（幂等：并发时可能已被别人删）
                }
            }
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // 原子：EEXIST ⇒ 已被持锁
            .open(&path)
            .map_err(|e| {
                let holder = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|c| LockInfo::parse(&c))
                    .map(|i| format!(" (held by pid={} since {})", i.pid, i.ts))
                    .unwrap_or_default();
                anyhow::anyhow!(
                    "rebuild lock already held: {}{holder} — another rebuild is in progress (or a crashed one left a stale lock)",
                    path.display()
                )
                .context(format!("acquire rebuild lock {}: {e}", path.display()))
            })?;
        let info = LockInfo {
            pid: std::process::id(),
            ts: chrono::Utc::now().to_rfc3339(),
            mode: mode.to_string(),
        };
        f.write_all(info.to_line().as_bytes())?;
        f.flush()?;
        drop(f);
        Ok(RebuildLock { path })
    }
}

impl Drop for RebuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 服务面用的「重建中」判定：是否应拒绝本次请求。
/// 返回 Some(reason 文本) ⇒ 该请求应当被拒（503 + Retry-After）。
pub fn block_reason(home: &Path) -> Option<String> {
    let info = inspect(home)?;
    Some(format!(
        "rebuild in progress (pid={}, since {}) — 派生索引正在重建，写入/读取已暂时拒绝；重建结束后自动恢复",
        info.pid, info.ts
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home(tag: &str) -> PathBuf {
        let h = std::env::temp_dir().join(format!("ehr-lock-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(&h).unwrap();
        h
    }

    #[test]
    fn acquire_then_second_acquire_fails_and_release_unlocks() {
        let home = tmp_home("acq");
        let l1 = RebuildLock::acquire(&home, "rebuild").unwrap();
        assert!(lock_path(&home).exists());
        // 第二个取锁必须失败（互斥生效）
        let e2 = RebuildLock::acquire(&home, "rebuild").unwrap_err();
        assert!(format!("{e2:#}").contains("already held"), "{e2:#}");
        // inspect 应报「有人在重建」
        assert!(inspect(&home).is_some());
        assert!(block_reason(&home).is_some());
        // 释放后可以再取
        drop(l1);
        assert!(!lock_path(&home).exists());
        assert!(inspect(&home).is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn stale_lock_from_dead_pid_is_reclaimed() {
        let home = tmp_home("stale");
        // 造一个指向不可能存活 pid 的锁（pid=999999 通常不存在；用极大值降低撞车概率）
        std::fs::write(
            lock_path(&home),
            format!("pid={}\nts=2020-01-01T00:00:00Z\nmode=rebuild\n", u32::MAX),
        )
        .unwrap();
        // 服务面：陈旧锁不构成阻塞
        assert!(inspect(&home).is_none());
        assert!(block_reason(&home).is_none());
        // 取锁方：陈旧锁被回收 ⇒ 取锁成功
        let l = RebuildLock::acquire(&home, "rebuild").unwrap();
        assert!(lock_path(&home).exists());
        drop(l);
        let _ = std::fs::remove_dir_all(&home);
    }
}
