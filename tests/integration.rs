use std::io::Write;
use std::process::Command;

fn fush(script: &str) -> (String, String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .arg("-c")
        .arg(script)
        .output()
        .expect("failed to run fush");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&stderr_bytes(&out)).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn stderr_bytes(out: &std::process::Output) -> Vec<u8> {
    out.stderr.clone()
}

#[test]
fn echo_hello() {
    let (out, err, code) = fush("echo hello");
    assert_eq!(out, "hello\n");
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn list_expansion_separate_words() {
    let (out, _, _) = fush("set x a b; echo $x");
    assert_eq!(out, "a b\n");
}

#[test]
fn undefined_var_is_zero_words() {
    let (out, _, _) = fush("echo pre$missing post");
    assert_eq!(out, "post\n");
}

#[test]
fn bare_concat_cartesian_product() {
    let (out, _, _) = fush("set x a b; set y c d; echo $x$y");
    assert_eq!(out, "ac bc ad bd\n");
}

#[test]
fn bare_concat_zero_value_drops_word() {
    let (out, _, _) = fush("set y; echo a$y");
    assert_eq!(out, "\n");
}

#[test]
fn quoted_concat_is_one_word() {
    let (out, _, _) = fush("set x a b; echo \"v=$x!\"");
    assert_eq!(out, "v=a b!\n");
}

#[test]
fn bare_concat_with_undefined_value_is_empty() {
    let (out, _, _) = fush("echo a$missing");
    assert_eq!(out, "\n");
}

#[test]
fn cmdsubst_capture_and_split() {
    let (out, _, _) = fun();
    assert_eq!(out, "one two\n");
    fn fun() -> (String, String, i32) {
        fush("echo (echo one; echo two)")
    }
}

#[test]
fn cmdsubst_in_concat() {
    let (out, _, _) = fush("echo x(echo mid)y");
    assert_eq!(out, "x mid y\n");
}

#[test]
fn local_scope_shadow() {
    let (out, _, _) = fush("set y outer; set -l y inner; echo $y");
    assert_eq!(out, "inner\n");
}

#[test]
fn exported_var_visible_to_children() {
    let (out, _, _) = fush("set -x FUSH_TEST_VAR hello42; sh -c 'echo $FUSH_TEST_VAR'");
    assert_eq!(out, "hello42\n");
}

#[test]
fn pipeline_two_stage() {
    let (out, _, _) = fush("echo hi | wc -c");
    assert_eq!(out.trim(), "3");
}

#[test]
fn pipeline_three_stage() {
    let (out, _, _) = fush("printf 'b\\na\\nc\\n' | sort | head -n1");
    assert_eq!(out, "a\n");
}

#[test]
fn builtin_piped_to_external() {
    let (out, _, _) = fush("echo hello | tr a-z A-Z");
    assert_eq!(out, "HELLO\n");
}

#[test]
fn silent_builtin_mid_pipeline_is_empty_stream() {
    let (out, _, _) = fush("echo hi | true; echo $status");
    assert_eq!(out, "0\n");
}

#[test]
fn set_builtin_mid_pipeline_is_empty_stream() {
    let (out, _, _) = fush("set x ok | true; echo $x $status");
    assert_eq!(out, "ok 0\n");
}

#[test]
fn pipeline_status_is_last_stage() {
    let (out, _, _) = fush("echo hi | grep -q zzz; echo $status");
    assert_eq!(out, "1\n");
}

#[test]
fn status_after_failure() {
    let (out, _, _) = fush("false; echo $status");
    assert_eq!(out, "1\n");
}

#[test]
fn status_after_success() {
    let (out, _, _) = fush("true; echo $status");
    assert_eq!(out, "0\n");
}

#[test]
fn not_negates_status() {
    let (out, _, _) = fush("not false; echo $status");
    assert_eq!(out.trim(), "0");
    let (out, _, _) = fush("not true; echo $status");
    assert_eq!(out, "1\n");
}

#[test]
fn and_chain_runs_on_success() {
    let (out, _, _) = fush("true and echo ran");
    assert_eq!(out, "ran\n");
    let (out, _, _) = fush("false and echo ran");
    assert_eq!(out, "");
}

#[test]
fn or_chain_runs_on_failure() {
    let (out, _, _) = fush("false or echo ran");
    assert_eq!("ran\n", out);
    let (out, _, _) = fush("true or echo ran");
    assert_eq!("", out);
}

#[test]
fn block_chain_follows_if_truthiness() {
    let (out, _, code) = fush("if false; echo a; end; and echo yes");
    assert_eq!(out, "yes\n");
    assert_eq!(code, 0);

    let (out, _, code) = fush("if true; echo a; end; and echo yes");
    assert_eq!(out, "a\nyes\n");
    assert_eq!(code, 0);

    let (out, _, code) = fush("if true; false; end; and echo yes");
    assert_eq!(out, "");
    assert_eq!(code, 1);

    let (out, _, code) = fush("if true; false; end; or echo backup");
    assert_eq!(out, "backup\n");
    assert_eq!(code, 0);

    let (out, _, code) = fush("if false; echo a; end; and echo yes; or echo backup");
    assert_eq!(out, "yes\n");
    assert_eq!(code, 0);
}

#[test]
fn function_and_while_switch_semantics_follow_block_status() {
    let (out, _, code) = fush("function f; end; and echo x");
    assert_eq!(out, "x\n");
    assert_eq!(code, 0);

    let (out, _, code) = fush("while false; end; or echo no");
    assert_eq!(out, "");
    assert_eq!(code, 0);

    let (out, _, code) = fush("while false; end; echo $status");
    assert_eq!(out, "0\n");
    assert_eq!(code, 0);

    let (out, _, code) = fush("for i in; end; echo $status");
    assert_eq!(out, "0\n");
    assert_eq!(code, 0);

    let (out, _, code) = fush("switch x; case y; false; end; echo $status");
    assert_eq!(out, "0\n");
    assert_eq!(code, 0);

    let (out, _, code) = fush("if false; echo a; end; echo $status");
    assert_eq!(out, "0\n");
    assert_eq!(code, 0);
}

#[test]
fn if_else_branches() {
    let (out, _, _) = fush("if test 1 -eq 1; echo yes; else; echo no; end");
    assert_eq!(out, "yes\n");
    let (out, _, _)

 = fush("if test 1 -eq 2; echo yes; else; echo no; end");
    assert_eq!(out, "no\n");
}

#[test]
fn for_iterates() {
    let (out, _, _) = fush("for i in a b c; echo $i; end");
    assert_eq!(out, "a\nb\nc\n");
}

#[test]
fn for_var_scoped() {
    let (out, _, _) = fush("set i keep; for i in x; echo $i; end; echo $i");
    assert_eq!(out, "x\nkeep\n");
}

#[test]
fn while_loops() {
    let (out, _, _) = fush("set n 0; while test $n -lt 3; set n (expr $n + 1); end; echo $n");
    assert_eq!(out.split_whitespace().last(), Some("3"));
}

#[test]
fn repl_via_piped_stdin_echo_and_exit() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fush"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn fush");
    child.stdin.as_mut().unwrap().write_all(b"echo hi\nexit\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn repl_multiline_block_via_piped_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fush"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn fush");
    child.stdin.as_mut().unwrap().write_all(b"if test 1 -eq 1\n  echo ok\nend\nexit\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn repl_unknown_command_keeps_running_after_error() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fush"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn fush");
    child.stdin.as_mut().unwrap().write_all(b"nope\necho ok\nexit\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok\n"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("fush:"));
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn switch_matches_case() {
    let (out, _, _) = fush("switch b; case a c; echo first; case b; echo second; end");
    assert_eq!(out, "second\n");
}

#[test]
fn switch_multi_pattern() {
    let (out, _, _) = fush("switch c; case a b c; echo hit; case '*'; echo miss; end");
    assert_eq!(out, "hit\n");
}

#[test]
fn switch_glob_star() {
    let (out, _, _) = fush("switch xyz; case a?c; echo one; case x*; echo two; end");
    assert_eq!(out, "two\n");
}

#[test]
fn switch_glob_question() {
    let (out, _, _) = fush("switch abc; case a?c; echo hit; end");
    assert_eq!(out, "hit\n");
}

#[test]
fn switch_multibyte_glob_matches_char_wise() {
    let (out, _, _) = fush("switch café; case *é*; echo hit; end");
    assert_eq!(out, "hit\n");
}

#[test]
fn switch_cmdsubst_runs_once() {
    let (out, _, _) = fush("switch (echo one); case one; echo matched; end");
    assert_eq!(out, "matched\n");
}

#[test]
fn switch_multiword_value_errors_like_fish() {
    let (_, err, code) = fush("set x a b; switch $x; case a; echo A; case b; echo B; end");
    assert_eq!(code, 1);
    assert!(err.contains("expanded to 2 words, expected one"));
}

#[test]
fn redirect_out_and_in() {
    let tmp = std::env::temp_dir().join("fush-it-redir.txt");
    let _ = std::fs::remove_file(&tmp);
    let tmps = tmp.to_string_lossy().into_owned();
    let (out, _, code) = fush(&format!("echo saved > {tmps}; cat < {tmps}"));
    assert_eq!(code, 0);
    assert_eq!(out, "saved\n");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn exit_code_propagates() {
    let (_, _, code) = fush("exit 7");
    assert_eq!(code, 7);
}

#[test]
fn parse_error_reports() {
    let (_, err, code) = fush("if test 1; echo x");
    assert_eq!(code, 1);
    assert!(err.starts_with("fush: "));
}

#[test]
fn unknown_command_errors() {
    let (_, err, _) = fush("fush-definitely-not-a-command-xyz");
    assert!(err.contains("fush: failed to run"));
}

#[test]
fn cd_and_pwd() {
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    let (out, _, code) = fush(&format!("cd {tmp}; echo $PWD"));
    assert_eq!(code, 0);
    assert!(out.trim().starts_with("/"));
}

fn tmpfile(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("fush-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.join(name).to_str().expect("utf8").to_string()
}

#[test]
fn external_redirect_out_writes_file() {
    let f = tmpfile("out.txt");
    let (out, _, code) = fush(&format!("sh -c 'echo hi' > {f}"));
    assert_eq!(out, "");
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "hi\n");
}

#[test]
fn external_redirect_err_writes_file() {
    let f = tmpfile("err.txt");
    let (out, _, code) = fush(&format!("sh -c 'echo boom >&2' 2> {f}"));
    assert_eq!(out, "");
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "boom\n");
}

#[test]
fn external_redirect_in_reads_file() {
    let f = tmpfile("in.txt");
    std::fs::write(&f, "line1\nline2\n").unwrap();
    let (out, _, code) = fush(&format!("cat < {f}"));
    assert_eq!(out, "line1\nline2\n");
    assert_eq!(code, 0);
}

#[test]
fn and_after_semicolon_runs_on_success() {
    let (out, _, code) = fush("true; and echo yes");
    assert_eq!(out, "yes\n");
    assert_eq!(code, 0);
}

#[test]
fn and_after_semicolon_skips_on_failure() {
    let (out, _, code) = fush("false; and echo no");
    assert_eq!(out, "");
    assert_eq!(code, 1);
}

#[test]
fn or_after_semicolon_runs_on_failure() {
    let (out, _, code) = fush("false; or echo backup");
    assert_eq!(out, "backup\n");
    assert_eq!(code, 0);
}

#[test]
fn and_after_newline_chains() {
    let (out, _, code) = fush("true\nand echo multi");
    assert_eq!(out, "multi\n");
    assert_eq!(code, 0);
}

#[test]
fn chained_or_then_and_after_separators() {
    let (out, _, code) = fush("false; or echo one; and echo two");
    assert_eq!(out, "one\ntwo\n");
    assert_eq!(code, 0);
}

#[test]
fn quoted_cmdsubst_is_single_word_with_newlines() {
    let (out, _, code) = fush(r#"echo "a(echo one; echo two)b""#);
    assert_eq!(out, "aone\ntwob\n");
    assert_eq!(code, 0);
}

#[test]
fn unquoted_cmdsubst_still_word_splits() {
    let (out, _, code) = fush("echo (echo one; echo two)");
    assert_eq!(out, "one two\n");
    assert_eq!(code, 0);
}

#[test]
fn dash_c_without_argument_errors() {
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .arg("-c")
        .output()
        .expect("failed to run fush");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("-c requires a command"));
}

#[test]
fn repl_incomplete_block_at_eof_reports_error() {
    let mut out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn fush");
    {
    use std::io::Write;
    let mut sin = out.stdin.take().expect("stdin");
    sin.write_all(b"if true\nexit\n").expect("write");
    }
    let done = out.wait_with_output().expect("wait");
    assert_eq!(done.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&done.stderr).contains("expected 'end'"));
    assert_eq!(String::from_utf8_lossy(&done.stdout), "");
}

#[test]
fn repl_incomplete_block_completed_by_more_input() {
    let mut out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn fush");
    {
    use std::io::Write;
    let mut sin = out.stdin.take().expect("stdin");
    sin.write_all(b"if true\necho deep\nend\necho done\nexit\n").expect("write");
    }
    let done = out.wait_with_output().expect("wait");
    assert_eq!(done.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&done.stdout), "deep\ndone\n");
}

#[test]
fn repl_parse_error_then_continue() {
    let mut out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn fush");
    {
    use std::io::Write;
    let mut sin = out.stdin.take().expect("stdin");
    sin.write_all(b"echo ) bad\necho ok\nexit\n").expect("write");
    }
    let done = out.wait_with_output().expect("wait");
    assert_eq!(done.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&done.stderr).contains("fush:"));
    assert_eq!(String::from_utf8_lossy(&done.stdout), "ok\n");
}

fn fush_with_config(config: &str, args: &[&str]) -> (String, String, i32) {
    let dir = std::env::temp_dir().join(format!("fush-config-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let cfg = dir.join("config.fish");
    std::fs::write(&cfg, config).expect("write config");
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(args)
        .env("FUSH_CONFIG", &cfg)
        .output()
        .expect("run fush");
    let _ = std::fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn config_var_visible_in_c_mode() {
    let (out, err, code) = fush_with_config("set greeting hello", &["-c", "echo $greeting"]);
    assert_eq!(out, "hello\n");
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn config_function_callable_in_c_mode() {
    let (out, _, _) = fush_with_config(
        "function greet\n    echo hi from fn\nend",
        &["-c", "greet"],
    );
    assert_eq!(out, "hi from fn\n");
}

#[test]
fn config_missing_is_silent() {
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(["-c", "echo ok"])
        .env("FUSH_CONFIG", "/nonexistent/fush/config.fish")
        .output()
        .expect("run fush");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
    assert_eq!(out.stderr.len(), 0);
}

#[test]
fn config_parse_error_shell_continues() {
    let (out, err, code) = fush_with_config("echo ) bad", &["-c", "echo after"]);
    assert!(err.contains("fush: config:"));
    assert_eq!(out, "after\n");
    assert_eq!(code, 0);
}

#[test]
fn config_failing_command_shell_continues() {
    let (out, err, code) = fush_with_config(
        "set greeting ok\ncd /nonexistent/dir",
        &["-c", "echo $greeting"],
    );
    assert!(err.contains("fush: cd:"));
    assert_eq!(out, "ok\n");
    assert_eq!(code, 0);
}

#[test]
fn config_abbr_reaches_tty_repl() {
    let dir = std::env::temp_dir().join(format!("fush-abbr-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let cfg = dir.join("config.fish");
    std::fs::write(&cfg, "abbr gs git status").expect("write config");
    let mut out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("FUSH_CONFIG", &cfg)
        .spawn()
        .expect("spawn fush");
    {
        use std::io::Write;
        let mut sin = out.stdin.take().expect("stdin");
        sin.write_all(b"echo $gs\nexit\n").expect("write");
    }
    let done = out.wait_with_output().expect("wait");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(done.status.code(), Some(0));
}

fn fush_with_universals(
    universals: &str,
    config: Option<&str>,
    args: &[&str],
) -> (String, String, i32) {
    let dir = std::env::temp_dir().join(format!(
        "fush-uni-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let uni = dir.join("universals");
    std::fs::write(&uni, universals).expect("write universals");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fush"));
    cmd.args(args).env("FUSH_UNIVERSALS", &uni);
    if let Some(cfg) = config {
        let cfg_path = dir.join("config.fish");
        std::fs::write(&cfg_path, cfg).expect("write config");
        cmd.env("FUSH_CONFIG", &cfg_path);
    }
    let out = cmd.output().expect("run fush");
    let _ = std::fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn universals_visible_in_c_mode() {
    let (out, err, code) = fush_with_universals("greeting\thello\n", None, &["-c", "echo \"$greeting\""]);
    assert_eq!(out, "hello\n");
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn universals_multi_value_in_c_mode() {
    let (out, _, _) = fush_with_universals("list\ta\tb c\td\n", None, &["-c", "echo \"$list\""]);
    assert_eq!(out, "a b c d\n");
}

#[test]
fn universals_load_before_config() {
    // Config runs after universals load, so config can override a
    // universal for the session and can see universals set earlier.
    let (out, _, _) = fush_with_universals(
        "greeting\thello\n",
        Some("set greeting goodbye"),
        &["-c", "echo \"$greeting\""],
    );
    assert_eq!(out, "goodbye\n");
    let (out, _, _) = fush_with_universals(
        "greeting\thello\n",
        Some("echo cfg=\"$greeting\""),
        &["-c", "true"],
    );
    assert_eq!(out, "cfg=hello\n");
}

#[test]
fn universals_corrupt_lines_skipped_with_one_warning() {
    let (out, err, code) = fush_with_universals(
        "good\t1\n\n\tbad\nalsogood\t2\n",
        None,
        &["-c", "echo \"$good $alsogood\""],
    );
    assert_eq!(out, "1 2\n");
    assert_eq!(code, 0);
    assert_eq!(err.matches("fush: warning:").count(), 1);
    assert!(err.contains("malformed line"));
}

#[test]
fn set_u_persists_across_processes() {
    let dir = std::env::temp_dir().join(format!(
        "fush-uni-persist-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let uni = dir.join("universals");
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(["-c", "set -U greeting hello multi"])
        .env("FUSH_UNIVERSALS", &uni)
        .output()
        .expect("run fush 1");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&uni).unwrap(), "greeting\thello\tmulti\n");
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(["-c", "echo \"$greeting\""])
        .env("FUSH_UNIVERSALS", &uni)
        .output()
        .expect("run fush 2");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello multi\n");
}

#[test]
fn set_e_erases_universal_across_processes() {
    let dir = std::env::temp_dir().join(format!(
        "fush-uni-erase-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let uni = dir.join("universals");
    std::fs::write(&uni, "greeting\thello\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(["-c", "set -e greeting; echo \"gone=$greeting\""])
        .env("FUSH_UNIVERSALS", &uni)
        .output()
        .expect("run fush");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "gone=\n");
    assert_eq!(std::fs::read_to_string(&uni).unwrap(), "");
    let _ = std::fs::remove_dir_all(&dir);
}

fn fush_with_fush_config(fush_cfg: Option<&str>, fish_cfg: Option<&str>, args: &[&str]) -> (String, String, i32) {
    let dir = std::env::temp_dir().join(format!(
        "fush-fushcfg-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fush"));
    cmd.args(args);
    if let Some(cfg) = fush_cfg {
        let p = dir.join("config.fush");
        std::fs::write(&p, cfg).expect("write fush config");
        cmd.env("FUSH_CONFIG", &p);
    }
    if let Some(cfg) = fish_cfg {
        let p = dir.join("config.fish");
        std::fs::write(&p, cfg).expect("write fish config");
        cmd.env("FUSH_FISH_CONFIG", &p);
    }
    let out = cmd.output().expect("run fush");
    let _ = std::fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn fush_config_wins_over_fish_config_in_c_mode() {
    let (out, err, code) = fush_with_fush_config(
        Some("set x 1"),
        Some("set x 2"),
        &["-c", "echo $x"],
    );
    assert_eq!(out, "1\n");
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn c_mode_without_fush_config_never_prompts() {
    let (out, err, code) = fush_with_fush_config(None, Some("set x 2"), &["-c", "echo ok"]);
    assert_eq!(out, "ok\n");
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn set_ul_rejected_in_c_mode() {
    let dir = std::env::temp_dir().join(format!(
        "fush-uni-ul-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let uni = dir.join("universals");
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(["-c", "set -Ul x 1"])
        .env("FUSH_UNIVERSALS", &uni)
        .output()
        .expect("run fush");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not supported"));
}

fn fush_with_home(home: &std::path::Path, args: &[&str]) -> (String, String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(args)
        .env("HOME", home)
        .env_remove("USERPROFILE")
        .output()
        .expect("run fush");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn profile_home(name: Option<&str>, profile_body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fush-profile-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    if let Some(name) = name {
        let pdir = dir.join(".config").join("fush").join("profiles");
        std::fs::create_dir_all(&pdir).expect("mkdir profiles");
        std::fs::write(pdir.join(format!("{name}.fush")), profile_body).expect("write profile");
    } else {
        std::fs::create_dir_all(&dir).expect("mkdir home");
    }
    dir
}

#[test]
fn read_only_profile_blocks_external() {
    let home = profile_home(None, "");
    let (out, err, code) = fush_with_home(&home, &["--profile", "-c", "sh -c 'echo hi'"]);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(out, "");
    assert!(err.contains("not allowed in this profile: sh"));
    assert_eq!(code, 1);
}

#[test]
fn read_only_profile_builtins_free() {
    let home = profile_home(None, "");
    let (out, err, code) = fush_with_home(&home, &["--profile", "-c", "echo hi; set x 1; test $x = 1; and echo ok"]);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(out, "hi\nok\n");
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn named_profile_allowlisted_external_runs() {
    let home = profile_home(Some("dev"), "profile allow sh");
    let (out, err, code) = fush_with_home(&home, &["--profile", "dev", "-c", "sh -c 'echo hi'"]);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(out, "hi\n");
    assert_eq!(err, "");
    assert_eq!(code, 0);
}

#[test]
fn named_profile_unlisted_external_blocked() {
    let home = profile_home(Some("dev"), "profile allow sh");
    let (out, err, code) = fush_with_home(&home, &["--profile", "dev", "-c", "git status"]);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(out, "");
    assert!(err.contains("not allowed in this profile: git"));
    assert_eq!(code, 1);
}

#[test]
fn read_only_profile_write_redirect_blocked() {
    let home = profile_home(None, "");
    let work = std::env::temp_dir().join(format!("fush-wr-{}-{}", std::process::id(), std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let target = work.join("out.txt");
    let (_, err, code) = fush_with_home(
        &home,
        &["--profile", "-c", &format!("echo hi > {}", target.display())],
    );
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&work);
    assert!(!target.exists());
    assert!(err.contains("not allowed in this profile: write to"));
    assert_eq!(code, 1);
}

#[test]
fn named_profile_write_redirect_allowed_in_allowlisted_dir() {
    let home = profile_home(Some("dev"), "profile allow sh\nprofile allow-write /tmp");
    let work = std::env::temp_dir().join(format!("fush-wa-{}-{}", std::process::id(), std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let target = work.join("out.txt");
    let (_, err, code) = fush_with_home(
        &home,
        &["--profile", "dev", "-c", &format!("echo hi > {}", target.display())],
    );
    let contents = std::fs::read_to_string(&target).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&work);
    assert_eq!(err, "");
    assert_eq!(code, 0);
    assert_eq!(contents, "hi\n");
}

#[test]
fn read_only_profile_cmdsubst_inner_blocked() {
    let home = profile_home(None, "");
    let (out, err, _code) = fush_with_home(&home, &["--profile", "-c", "echo (sh -c 'echo hi')"]);
    let _ = std::fs::remove_dir_all(&home);
    assert!(err.contains("not allowed in this profile: sh"));
    assert!(!out.contains("hi"));
}

#[test]
fn read_only_profile_builtin_echo_redirect_blocked() {
    let home = profile_home(None, "");
    let work = std::env::temp_dir().join(format!("fush-wb-{}-{}", std::process::id(), std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let target = work.join("out.txt");
    let (_, err, code) = fush_with_home(
        &home,
        &["--profile", "-c", &format!("echo hi > {}", target.display())],
    );
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&work);
    assert!(!target.exists());
    assert!(err.contains("not allowed in this profile: write to"));
    assert_eq!(code, 1);
}

#[test]
fn read_only_profile_cd_free() {
    let home = profile_home(None, "");
    let work = std::env::temp_dir().join(format!("fush-cd-{}-{}", std::process::id(), std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let (out, err, code) = fush_with_home(&home, &["--profile", "-c", &format!("cd {}; echo $status", work.display())]);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&work);
    assert_eq!(err, "");
    assert_eq!(code, 0);
    assert_eq!(out.trim_end(), "0");
}

// --- profiles end-to-end (Task 3): REPL path, write outside, missing profile, fish syntax ---

#[test]
fn profile_write_outside_allowlist_blocked() {
    let home = profile_home(
        Some("dev"),
        &format!("profile allow sh\nprofile allow-write /tmp/fush-allowed-{}-{ }", std::process::id(), 0),
    );
    let outside = std::env::temp_dir().join(format!("fush-outside-{}-{}.txt", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let (_, err, code) = fush_with_home(
        &home,
        &["--profile", "dev", "-c", &format!("echo hi > {}", outside.display())],
    );
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_file(&outside);
    assert!(!outside.exists());
    assert!(err.contains("not allowed in this profile: write to"), "err: {err}");
    assert_eq!(code, 1);
}

#[test]
fn missing_profile_fails_startup_exit_2_noninteractive() {
    let home = profile_home(None, "");
    let (_, err, code) = fush_with_home(&home, &["--profile", "ghost", "-c", "echo hi"]);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 2);
    assert!(err.contains("cannot read"), "err: {err}");
    assert!(!err.contains("hi"));
}

#[test]
fn missing_profile_fails_startup_exit_2_repl() {
    let home = profile_home(None, "");
    let mut child = Command::new(env!("CARGO_BIN_EXE_fush"))
        .args(["--profile", "ghost"])
        .env("HOME", &home)
        .env_remove("USERPROFILE")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn fush");
    child.stdin.take().unwrap().write_all(b"echo hi\n").unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

#[test]
fn profile_file_supports_full_fish_syntax() {
    let home = profile_home(Some("envy"), "set -x FOO bar\nprofile allow sh\n");
    let (out, err, code) = fush_with_home(
        &home,
        &["--profile", "envy", "-c", "sh -c 'echo $FOO'"],
    );
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 0, "err: {err}");
    assert_eq!(out, "bar\n");
}
