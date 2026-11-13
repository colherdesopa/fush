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
    assert_eq!(out, "pre post\n");
}

#[test]
fn quoted_concat_is_one_word() {
    let (out, _, _) = fush("set x a b; echo \"v=$x!\"");
    assert_eq!(out, "v=a b!\n");
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
