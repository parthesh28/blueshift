use pinocchio::program_error::ProgramError;

#[derive(Clone, PartialEq)]
pub enum PinocchioError {
    NotSigner,
    InvalidOwner,
    InvalidAccountData,
    InvalidAddress,
}

impl From<PinocchioError> for ProgramError {
    fn from(e: PinocchioError) -> Self {
        Self::Custom(e as u32)
    }
}