use anyhow::{bail, Context, Result};
use log::debug;
use run_script::ScriptOptions;

pub struct Shell<S: AsRef<str>>(pub(super) S);

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
            .and_then(|(code, output, error)| {
                let res = f(code, &output, &error);
                res.with_context(|| {
                    format!("\n\tshell script:\n{}\n\texit code: {code}\n\tstdout: {output}\n\tstderr: {error}", self.0.as_ref())
                })
            })
    }
}
