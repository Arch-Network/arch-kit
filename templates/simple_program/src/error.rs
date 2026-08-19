use arch_satellite_lang::prelude::*;

#[error_code]
pub enum HelloWorldError {
    #[msg("The user must sign the say_hello instruction")]
    UserMustSign,
}
