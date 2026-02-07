use pinocchio::{
    account_info::{AccountInfo, RefMut},
    instruction::{Seed, Signer},
    program_error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;
use pinocchio_token::{instructions::InitializeMint2, state::Mint};
use std::mem::MaybeUninit;

use crate::Config;

pub struct InitializeAccounts<'a> {
    pub initializer: &'a AccountInfo,
    pub mint_lp: &'a AccountInfo,
    pub config: &'a AccountInfo,
}

impl<'a> TryFrom<&'a [AccountInfo]> for InitializeAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let [initializer, mint_lp, config, _system_program, _token_program] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };
        Ok(Self {
            initializer,
            mint_lp,
            config,
        })
    }
}

#[repr(C, packed)]
pub struct InitializeInstructionData {
    pub seed: u64,
    pub fee: u16,
    pub mint_x: [u8; 32],
    pub mint_y: [u8; 32],
    pub config_bump: [u8; 1],
    pub lp_bump: [u8; 1],
    pub authority: [u8; 32],
}

impl TryFrom<&[u8]> for InitializeInstructionData {
    type Error = ProgramError;
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        const INITIALIZE_DATA_LEN_WITH_AUTHORITY: usize = size_of::<InitializeInstructionData>();
        const INITIALIZE_DATA_LEN: usize =
            INITIALIZE_DATA_LEN_WITH_AUTHORITY - size_of::<[u8; 32]>();
        match data.len() {
            INITIALIZE_DATA_LEN_WITH_AUTHORITY => {
                Ok(unsafe { (data.as_ptr() as *const Self).read_unaligned() })
            }
            INITIALIZE_DATA_LEN => {
                let mut raw: MaybeUninit<[u8; INITIALIZE_DATA_LEN_WITH_AUTHORITY]> =
                    MaybeUninit::uninit();
                let raw_ptr = raw.as_mut_ptr() as *mut u8;
                unsafe {
                    core::ptr::copy_nonoverlapping(data.as_ptr(), raw_ptr, INITIALIZE_DATA_LEN);
                    core::ptr::write_bytes(raw_ptr.add(INITIALIZE_DATA_LEN), 0, 32);
                    Ok((raw.as_ptr() as *const Self).read_unaligned())
                }
            }
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }
}

pub struct Initialize<'a> {
    pub accounts: InitializeAccounts<'a>,
    pub instruction_data: InitializeInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountInfo])> for Initialize<'a> {
    type Error = ProgramError;
    fn try_from((data, accounts): (&'a [u8], &'a [AccountInfo])) -> Result<Self, Self::Error> {
        let accounts = InitializeAccounts::try_from(accounts)?;
        let instruction_data = InitializeInstructionData::try_from(data)?;
        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Initialize<'a> {
    pub const DISCRIMINATOR: &'a u8 = &0;

    pub fn process(&mut self) -> ProgramResult {
        let seed_bindings = self.instruction_data.seed.to_le_bytes();
        let config_seeds = [
            Seed::from(b"config"),
            Seed::from(&seed_bindings),
            Seed::from(&self.instruction_data.mint_x),
            Seed::from(&self.instruction_data.mint_y),
            Seed::from(&self.instruction_data.config_bump),
        ];
        let signer = [Signer::from(&config_seeds)];

        CreateAccount {
            from: self.accounts.initializer,
            to: self.accounts.config,
            lamports: Rent::get()?.minimum_balance(Config::LEN),
            space: Config::LEN as u64,
            owner: &crate::ID,
        }
        .invoke_signed(&signer)?;

        let config_account = self.accounts.config;
        let mut config: RefMut<Config> = Config::load_mut(config_account)?;

        config.set_inner_data(
            self.instruction_data.seed,
            self.instruction_data.authority,
            self.instruction_data.mint_x,
            self.instruction_data.mint_y,
            self.instruction_data.fee,
            self.instruction_data.config_bump,
        )?;

        let mint_lp_seeds = [
            Seed::from(b"mint_lp"),
            Seed::from(self.accounts.config.key()),
            Seed::from(&self.instruction_data.lp_bump),
        ];
        let signer = [Signer::from(&mint_lp_seeds)];

        CreateAccount {
            from: self.accounts.initializer,
            to: self.accounts.mint_lp,
            lamports: Rent::get()?.minimum_balance(Mint::LEN),
            space: Mint::LEN as u64,
            owner: &pinocchio_token::ID,
        }
        .invoke_signed(&signer)?;

        InitializeMint2 {
            mint: self.accounts.mint_lp,
            decimals: 6,
            mint_authority: self.accounts.config.key(),
            freeze_authority: None,
        }
        .invoke_signed(&signer)?;

        Ok(())
    }
}