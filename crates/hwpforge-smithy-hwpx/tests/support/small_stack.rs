pub(crate) fn small_stack_child(test: &str, body: impl FnOnce() + Send + 'static) {
    const FLAG: &str = "HWPFORGE_WALK_FIX1_CHILD";
    if std::env::var(FLAG).as_deref() == Ok(test) {
        std::thread::Builder::new().stack_size(512 * 1024).spawn(body).unwrap().join().unwrap();
    } else {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env(FLAG, test)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed"));
    }
}
