use std::fmt;

/// Stable exit codes, documented in the README.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    General = 1,
    InvalidConfig = 2,
    Unsupported = 3,
    Resources = 4,
    Runtime = 5,
    Security = 6,
    Cancelled = 7,
    Download = 8,
}

/// An error with a user-facing hint and an exit code.
#[derive(Debug)]
pub struct CliError {
    pub code: Exit,
    pub msg: String,
    pub hint: Option<String>,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for CliError {}

impl CliError {
    pub fn new(code: Exit, msg: impl Into<String>) -> Self {
        Self {
            code,
            msg: msg.into(),
            hint: None,
        }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

pub fn fail_hint<T>(code: Exit, msg: impl Into<String>, hint: impl Into<String>) -> anyhow::Result<T> {
    Err(CliError::new(code, msg).hint(hint).into())
}

/// Maps any error to (exit code, hint), looking through the anyhow chain.
pub fn classify(err: &anyhow::Error) -> (Exit, Option<String>) {
    for cause in err.chain() {
        if let Some(e) = cause.downcast_ref::<CliError>() {
            return (e.code, e.hint.clone());
        }
        if let Some(e) = cause.downcast_ref::<inquire::InquireError>() {
            return match e {
                inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
                    (Exit::Cancelled, None)
                }
                inquire::InquireError::NotTTY => (
                    Exit::General,
                    Some("this step needs an interactive terminal; pass --yes or the matching flags".into()),
                ),
                _ => (Exit::General, None),
            };
        }
    }
    (Exit::General, None)
}
