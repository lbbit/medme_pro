// is_sha 形态闸的表驱动守卫。D5留疤:首版实现误用 is_ascii_lowercase()——std里它对 ASCII**数字
// 返回 false**(语义="是字母且小写")⇒ 一切含 0-9 的合法 sha 被自家 400 全灭(当轮 D/E 相雪崩、
// E2/E3假绿)。此测试先于 ehtest.sh 存在:换任何人重写都撞不回来(D4-b机制化判据)。
use ehr_api::vaults::is_sha;

#[test]
fn sha_gate_accepts_rejects_per_kernel_semantics() {
    let hex64 = "1a64ada443a41f911718a255390b26b06538c47a75611e760e2acaa569fb2f08"; // 真上传回值(含digit)
    assert!(is_sha(hex64), "digits must pass (the std trap)");
    assert!(is_sha(&"0".repeat(64)));
    assert!(is_sha(&"f".repeat(64)));

    assert!(!is_sha(&"A".repeat(64)), "uppercase rejected (canonical lowercase)");
    assert!(!is_sha("deadbeef"), "too short");
    let plus = format!("{hex64}ab");
    assert!(!is_sha(&plus), "+2 long");
    // D5不猜接口:get()对非法char界返回None而非panic(裸切片若将来样本改含多字节会炸测试本身)
    let minus = hex64.get(..63).unwrap();
    assert!(!is_sha(minus), "-1 short");
    assert!(!is_sha(&"g".repeat(64)), "non-hex letter");
    assert!(!is_sha(""), "empty");
}
