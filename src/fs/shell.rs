use anyhow::{bail, Context, Result};
use log::debug;
use run_script::ScriptOptions;

/// A wrapper around `run_script` that provides a convenient way to run shell commands.
pub struct Shell<S: AsRef<str>>(pub S);

impl<S: AsRef<str>> Shell<S> {
    pub fn run(self) -> Result<()> {
        self.run_with_status_checker(|code, _, _| {
            if code != 0 {
                bail!("Bad shell exit code")
            } else {
                Ok(())
            }
        })
    }

    pub fn run_with_status_checker<R>(self, f: impl Fn(i32, &str, &str) -> Result<R>) -> Result<R> {
        debug!("Running shell script:\n{}", self.0.as_ref());

        let mut ops = ScriptOptions::new();
        ops.exit_on_error = true;
        run_script::run_script!(self.0.as_ref(), ops)
            .map_err(Into::into)
            .and_then(|(code, stdout, stderr)| {
                let res = f(code, &stdout, &stderr);
                res.with_context(|| {
                    format!(
                        "\nshell script:\n{}\nexit code: {code}\nstdout: {}\nstderr: {}",
                        self.0.as_ref(),
                        if stdout.contains('\n') {
                            format!("(multi-line)\n\t{}", stdout.replace("\n", "\n\t"))
                        } else {
                            stdout
                        },
                        if stderr.contains('\n') {
                            format!("(multi-line)\n\t{}", stderr.replace("\n", "\n\t"))
                        } else {
                            stderr
                        },
                    )
                })
            })
    }
}
