use crate::ast::{ChainOp, Job, Redirect, RedirectMode, Stmt};
use crate::builtins::{run, BuiltinResult};
use crate::runtime::Runtime;
use std::io::Write;
use std::process::{Command as Child, Stdio};

/// Run a whole program. Returns the final $status.
pub fn run_program(rt: &mut Runtime, stmts: &[Stmt]) -> i32 {
    if let Err(e) = run_stmts(rt, stmts) {
        eprintln!("fush: {e}");
        rt.status = 1;
    }
    rt.status
}

/// Run one statement. Err is an error message; Ok(()) means execution
/// proceeded (status lives in rt.status).
pub fn run_stmt(rt: &mut Runtime, stmt: &Stmt) -> Result<(), String> {
    match stmt {
        Stmt::Job(job) => run_job(rt, job),
        Stmt::If { cond, then_body, else_body } => {
            run_job(rt, cond)?;
            if rt.status == 0 {
                run_stmts(rt, then_body)
            } else {
                run_stmts(rt, else_body)
            }
        }
        Stmt::For { var, items, body } => {
            let mut words = Vec::new();
            for item in items {
                words.extend(rt.expand(item)?);
            }
            rt.env.push_scope();
            for w in words {
                rt.env.set_local(var, vec![w]);
                run_stmts(rt, body)?;
                if rt.exited.is_some() {
                    break;
                }
            }
            rt.env.pop_scope();
            Ok(())
        }
        Stmt::While { cond, body } => loop {
            run_job(rt, cond)?;
            if rt.status != 0 || rt.exited.is_some() {
                break Ok(());
            }
            run_stmts(rt, body)?;
            if rt.exited.is_some() {
                break Ok(());
            }
        },
        Stmt::Switch { value, cases } => {
            let v = rt.expand_one(value)?;
            for case in cases {
                for pat in &case.patterns {
                    let p = rt.expand_one(pat)?;
                    if p == v || crate::runtime::glob_match(&p, &v) {
                        return run_stmts(rt, &case.body);
                    }
                }
            }
            Ok(())
        }
    }
}

pub fn run_stmts(rt: &mut Runtime, stmts: &[Stmt]) -> Result<(), String> {
    for stmt in stmts {
        run_stmt(rt, stmt)?;
        if rt.exited.is_some() {
            break;
        }
    }
    Ok(())
}

/// Run a job: pipeline, negation, and and/or chaining.
pub fn run_job(rt: &mut Runtime, job: &Job) -> Result<(), String> {
    let status = run_pipeline(rt, &job.pipeline)?;
    let mut st = if job.negated { negate(status) } else { status };
    rt.status = st;
    rt.env.set_global("status", vec![st.to_string()]);
    if let Some((op, rest)) = &job.chain {
        let proceed = match op {
            ChainOp::And => st == 0,
            ChainOp::Or => st != 0,
        };
        if proceed {
            run_job(rt, rest)?;
            st = rt.status;
        }
    }
    rt.status = st;
    Ok(())
}

fn negate(status: i32) -> i32 {
    if status == 0 {
        1
    } else {
        0
    }
}

/// Run a pipeline. External stages spawn sequentially with `output()`
/// (which waits for each child); stdout is chained between stages via
/// captured buffers. The last stage's exit status is the pipeline's.
///
/// Builtin-in-pipeline approach (simple, documented): builtins always run
/// in-process. Their stdout is captured to a buffer that becomes the next
/// stage's stdin; the final stage's stdout goes to the shell's stdout or
/// the active capture buffer (command substitution). This means external
/// stages do not run concurrently with builtins; for external-only
/// pipelines each stage still completes before the next starts, which is
/// acceptable for the PoC.
pub fn run_pipeline(rt: &mut Runtime, pipeline: &[crate::ast::Command]) -> Result<i32, String> {
    if pipeline.is_empty() {
        return Ok(rt.status);
    }

    let mut prev_out: Option<Vec<u8>> = None;
    let mut last_external_out: Option<Vec<u8>> = None;
    let mut status = rt.status;
    for (i, cmd) in pipeline.iter().enumerate() {
        let argv = expand_argv(rt, &cmd.args)?;
        let name = argv[0].clone();
        let is_last = i + 1 == pipeline.len();
        last_external_out = None;

        if let Some(res) = run(rt, &name, &argv[1..]) {
            match res {
                BuiltinResult::Exit(code) => {
                    rt.exited = Some(code);
                    return Ok(code);
                }
                BuiltinResult::Done { status: st, out } => {
                    status = st;
                    match out {
                        Some(bytes) => {
                            prev_out = emit_builtin_output(rt, cmd, bytes, is_last)?;
                        }
                        None => {
                            if !is_last {
                                return Err(format!(
                                    "builtin '{name}' produced no output for the pipe"
                                ));
                            }
                        }
                    }
                }
            }
        } else {
            let mut command = Child::new(&name);
            command.args(&argv[1..]);
            for (k, v) in rt.env.child_overrides() {
                command.env(k, v);
            }
            apply_redirects(rt, &mut command, &cmd.redirects)?;
            if let Some(input) = &prev_out {
                command.stdin(Stdio::piped());
                if !is_last {
                    command.stdout(Stdio::piped());
                }
                let mut child = command
                    .spawn()
                    .map_err(|e| format!("failed to run '{name}': {e}"))?;
                if let Some(mut sin) = child.stdin.take() {
                    let _ = sin.write_all(input);
                }
                last_external_out = None;
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                status = out.status.code().unwrap_or(1);
                if is_last {
                    last_external_out = Some(out.stdout);
                } else {
                    prev_out = Some(out.stdout);
                }
            } else {
                command.stdout(Stdio::piped());
                let out = command
                    .output()
                    .map_err(|e| format!("failed to run '{name}': {e}"))?;
                status = out.status.code().unwrap_or(1);
                if is_last {
                    last_external_out = Some(out.stdout);
                } else {
                    prev_out = Some(out.stdout);
                }
            }
        }
    }
    rt.status = status;
    if let Some(bytes) = last_external_out.take() {
        if let Some(buf) = &mut rt.capture {
            buf.extend_from_slice(&bytes);
        } else {
            let mut so = std::io::stdout();
            let _ = so.write_all(&bytes);
            let _ = so.flush();
        }
    }
    Ok(status)
}

/// Write builtin stdout to its destination: an Out redirect on fd 1, the
/// active capture buffer, or the shell's stdout. When the builtin's output
/// feeds a later pipeline stage, it is returned to the caller instead.
fn emit_builtin_output(
    rt: &mut Runtime,
    cmd: &crate::ast::Command,
    bytes: Vec<u8>,
    is_last: bool,
) -> Result<Option<Vec<u8>>, String> {
    for r in &cmd.redirects {
        if r.mode == RedirectMode::Out && r.fd == 1 {
            let target = rt.expand_one(&r.target)?;
            let mut f = std::fs::File::create(&target)
                .map_err(|e| format!("cannot redirect to '{target}': {e}"))?;
            f.write_all(&bytes).map_err(|e| e.to_string())?;
            return Ok(None);
        }
    }
    if is_last {
        if let Some(buf) = &mut rt.capture {
            buf.extend_from_slice(&bytes);
        } else {
            let mut so = std::io::stdout();
            let _ = so.write_all(&bytes);
            let _ = so.flush();
        }
        Ok(None)
    } else {
        Ok(Some(bytes))
    }
}

fn apply_redirects(
    rt: &mut Runtime,
    command: &mut Child,
    redirects: &[Redirect],
) -> Result<(), String> {
    for r in redirects {
        let target = rt.expand_one(&r.target)?;
        match (r.mode, r.fd) {
            (RedirectMode::In, 0) => {
                let f = std::fs::File::open(&target)
                    .map_err(|e| format!("cannot open '{target}' for reading: {e}"))?;
                command.stdin(Stdio::from(f));
            }
            (RedirectMode::Out, 1) => {
                let f = std::fs::File::create(&target)
                    .map_err(|e| format!("cannot open '{target}' for writing: {e}"))?;
                command.stdout(Stdio::from(f));
            }
            (RedirectMode::Out, 2) => {
                let f = std::fs::File::create(&target)
                    .map_err(|e| format!("cannot open '{target}' for writing: {e}"))?;
                command.stderr(Stdio::from(f));
            }
            (RedirectMode::In, fd) => {
                return Err(format!("redirect of fd {fd} for input not supported"));
            }
            (RedirectMode::Out, fd) => {
                return Err(format!("redirect of fd {fd} for output not supported"));
            }
        }
    }
    Ok(())
}

/// Expand command args into a full argv (command name first).
fn expand_argv(rt: &mut Runtime, args: &[crate::ast::Expr]) -> Result<Vec<String>, String> {
    let mut argv = Vec::new();
    for e in args {
        argv.extend(rt.expand(e)?);
    }
    if argv.is_empty() {
        return Err("empty command".to_string());
    }
    Ok(argv)
}
