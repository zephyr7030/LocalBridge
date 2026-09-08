use super::*;

fn fresh() -> MutexGuard<'static, ()> {
    lock_for_test()
}

#[test]
fn a_token_alone_is_not_permission() {
    let _guard = fresh();
    // 这是整个机制的要点：模型立刻拿到令牌，但令牌不是许可。
    // 若持有即可执行，"确认"就是模型自己发给自己的。
    let (token, _id) = request("shell", "format D: /q", None, &["disk_format"]);
    assert_eq!(
        redeem(&token, "shell", "format D: /q", None),
        Err(RedeemError::Awaiting)
    );
}

#[test]
fn an_approval_runs_the_command_once_and_only_that_command() {
    let _guard = fresh();
    let (token, id) = request(
        "shell",
        "del C:\\temp\\*",
        Some("C:\\work"),
        &["bulk_delete"],
    );
    assert!(approve(&id));

    // 批准的是眼前这一条，不是同一个令牌配上别的命令。
    assert_eq!(
        redeem(&token, "shell", "del C:\\*", Some("C:\\work")),
        Err(RedeemError::Mismatch)
    );
    // 工作目录也在摘要里：同一条命令换个目录是另一件事。
    assert_eq!(
        redeem(&token, "shell", "del C:\\temp\\*", Some("C:\\other")),
        Err(RedeemError::Mismatch)
    );
    // 路由同理。
    assert_eq!(
        redeem(&token, "process", "del C:\\temp\\*", Some("C:\\work")),
        Err(RedeemError::Mismatch)
    );

    assert_eq!(
        redeem(&token, "shell", "del C:\\temp\\*", Some("C:\\work")),
        Ok(())
    );
    // 一次批准只兑一次；重放要按"不存在"处理。
    assert_eq!(
        redeem(&token, "shell", "del C:\\temp\\*", Some("C:\\work")),
        Err(RedeemError::Unknown)
    );
}

#[test]
fn rejecting_makes_the_token_worthless_immediately() {
    let _guard = fresh();
    let (token, id) = request(
        "shell",
        "vssadmin delete shadows /all",
        None,
        &["boot_and_recovery"],
    );
    assert!(reject(&id));
    assert_eq!(
        redeem(&token, "shell", "vssadmin delete shadows /all", None),
        Err(RedeemError::Unknown)
    );
    assert!(awaiting().is_empty());
    // 同一个 id 不能拒第二次——它已经不在了。
    assert!(!reject(&id));
}

#[test]
fn an_expired_request_is_not_a_standing_permission() {
    let _guard = fresh();
    let (token, id) = request(
        "shell",
        "net user admin /add",
        None,
        &["administrator_accounts"],
    );
    assert!(approve(&id));
    force_expiry_for_test(&id);
    // 过期与不存在必须分开说：前者是"再问用户一次"，后者暗示调用方记错了。
    assert_eq!(
        redeem(&token, "shell", "net user admin /add", None),
        Err(RedeemError::Expired)
    );
    // 说完就该消失，第二次问按不存在处理。
    assert_eq!(
        redeem(&token, "shell", "net user admin /add", None),
        Err(RedeemError::Unknown)
    );
    // 过期条目也不该继续挂在待确认列表里等人看。
    assert!(awaiting().is_empty());
}

#[test]
fn an_unknown_token_and_a_spent_one_are_indistinguishable() {
    let _guard = fresh();
    assert_eq!(
        redeem("lb-confirm-nope", "shell", "whatever", None),
        Err(RedeemError::Unknown)
    );
}

#[test]
fn the_waiting_list_shows_what_a_person_needs_to_decide() {
    let _guard = fresh();
    let (_token, id) = request(
        "shell",
        "reg delete HKLM\\SOFTWARE\\Foo /f",
        Some("D:\\project"),
        &["registry_destruction"],
    );
    let pending = awaiting();
    assert_eq!(pending.len(), 1);
    let entry = &pending[0];
    assert_eq!(entry.id, id);
    // 命令原文一字不改地呈现——摘要过的命令没法让人做判断。
    assert_eq!(entry.command, "reg delete HKLM\\SOFTWARE\\Foo /f");
    assert_eq!(entry.workdir.as_deref(), Some("D:\\project"));
    assert_eq!(entry.risk, vec!["registry_destruction".to_string()]);
    assert!(entry.expires_at_ms > entry.requested_at_ms);

    // 已批准的不再占用待确认列表——它在等模型，不在等人。
    assert!(approve(&id));
    assert!(awaiting().is_empty());
}

#[test]
fn a_model_that_keeps_asking_cannot_grow_the_queue_without_bound() {
    let _guard = fresh();
    for index in 0..(MAX_PENDING + 8) {
        request(
            "shell",
            &format!("del C:\\{index}\\*"),
            None,
            &["bulk_delete"],
        );
    }
    assert!(awaiting().len() <= MAX_PENDING);
}

#[test]
fn queue_pressure_never_evicts_a_decision_a_person_already_made() {
    let _guard = fresh();
    let (token, id) = request("shell", "del C:\\keep\\*", None, &["bulk_delete"]);
    assert!(approve(&id));
    for index in 0..(MAX_PENDING + 8) {
        request(
            "shell",
            &format!("del C:\\{index}\\*"),
            None,
            &["bulk_delete"],
        );
    }
    // 已批准的那条还在，且仍可兑换。
    assert_eq!(redeem(&token, "shell", "del C:\\keep\\*", None), Ok(()));
}

#[test]
fn the_token_is_never_stored_in_the_clear() {
    let _guard = fresh();
    let (token, _id) = request("shell", "format D: /q", None, &["disk_format"]);
    let store = store();
    assert!(
        store.entries.iter().all(|entry| entry.token_hash != token),
        "the store kept the token itself, not its digest"
    );
    assert_eq!(store.entries[0].token_hash.len(), 64);
}

#[test]
fn every_change_moves_the_revision_so_the_window_knows_to_redraw() {
    let _guard = fresh();
    assert_eq!(revision(), 0);
    let (_token, id) = request("shell", "del C:\\a\\*", None, &["bulk_delete"]);
    let after_request = revision();
    assert!(after_request > 0);
    assert!(approve(&id));
    assert!(revision() > after_request);
}
