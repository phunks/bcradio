use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
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
    #[error("Phase error")]
    PhaseError,
}