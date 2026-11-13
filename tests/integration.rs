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
    // $missing is a bare var arg: zero words, so "pre" and "post" stay
    // separate arguments joined by echo's space
    assert_eq!(out, "pre post\n");
}

#[test]
fn quoted_concat_is_one_word() {
    let (out, _, _) = fush("set x a b; echo \"v=$x!\"");
    // concatenation joins values with spaces inside one word
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
    // the parser splits x(echo mid)y into three sibling args, not one Concat
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
    assert_eq!(out.trim().split_whitespace().last(), Some("3"));
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
