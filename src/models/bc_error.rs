use thiserror::Error;

#[derive(Error, Debug)]
pub enum BcradioError {
    #[error("Operation Interrupted")]
    OperationInterrupted,
    #[error("Invalid URL")]
    InvalidUrl,
    #[error("Canceled")]
    Cancel,
    #[error("Quit")]
    Quit,
    #[error("Could not resolve host")]
    CouldntResolveHost,
}