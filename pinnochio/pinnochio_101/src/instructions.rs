use core::convert::TryFrom;
use core::mem::size_of;
use pinocchio::{
    ProgramResult,
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::{Pubkey, find_program_address},
    sysvars::{Sysvar, rent::Rent},
};
use pinocchio_log::log;
use pinocchio_system::instructions::{CreateAccount, Transfer as SystemTransfer};

pub struct Deposit<'a> {
    pub owner: &'a AccountInfo,
    pub vault: &'a AccountInfo,
    pub amount: u64,
}

pub struct Withdraw<'a> {
    pub owner: &'a AccountInfo,
    pub vault: &'a AccountInfo,
}

fn parse_amount(data: &[u8]) -> Result<u64, ProgramError> {
    if data.len() != core::mem::size_of::<u64>() {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(data.try_into().unwrap());

    if amount == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }

    Ok(amount)
}

fn derive_vault(owner: &AccountInfo) -> (Pubkey, u8) {
    find_program_address(&[b"vault", owner.key().as_ref()], &crate::ID)
}

fn check_vault(owner: &AccountInfo, vault: &AccountInfo) -> ProgramResult {
    if !owner.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if vault.data_is_empty() {
        const ACCOUNT_DISCRIMINATOR_SIZE: usize = 8;

        let (expected_vault, _bump) = derive_vault(owner);

        // ensure the provided vault is same as the derived
        if vault.key() != &expected_vault {
            return Err(ProgramError::InvalidSeeds);
        }

        const VAULT_SIZE: usize = ACCOUNT_DISCRIMINATOR_SIZE + size_of::<u64>();
        let needed_lamports = Rent::get()?.minimum_balance(VAULT_SIZE);

        CreateAccount {
            from: owner,
            to: vault,
            lamports: needed_lamports,
            space: VAULT_SIZE as u64,
            owner: &crate::ID,
        }
        .invoke()?;

        log!("Vault created");
    } else {
        if !vault.is_owned_by(&crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        log!("Vault already exists");
    }

    Ok(())
}

impl<'a> Deposit<'a> {
    pub const DISCRIMINATOR: &'a u8 = &0;

    pub fn process(self) -> ProgramResult {
        let Deposit {
            owner,
            vault,
            amount,
        } = self;

        check_vault(owner, vault)?;

        SystemTransfer {
            from: owner,
            to: vault,
            lamports: amount,
        }
        .invoke()?;

        log!("{} Lamports deposited to vault", amount);
        Ok(())
    }
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountInfo])> for Deposit<'a> {
    type Error = ProgramError;

    fn try_from(value: (&'a [u8], &'a [AccountInfo])) -> Result<Self, Self::Error> {
        let (data, accounts) = value;
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        let owner = &accounts[0];
        let vault = &accounts[1];
        let system_program = &accounts[2];
        let amount = parse_amount(data)?;

        if system_program.key() != &pinocchio_system::ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        Ok(Self {
            owner,
            vault,
            amount,
        })
    }
}

impl<'a> Withdraw<'a> {
    pub const DISCRIMINATOR:&'a u8= &1;

    pub fn process(self) -> ProgramResult {
        let Withdraw { owner, vault } = self;

        if !owner.is_signer() {
            return Err(ProgramError::InvalidAccountOwner);
        }

        if !vault.is_owned_by(&crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        let (expected_vault_pda, _bump) = derive_vault(owner);
        if vault.key() != &expected_vault_pda {
            return Err(ProgramError::InvalidAccountData);
        }

        let data_len = vault.data_len();
        let min_balance = Rent::get()?.minimum_balance(data_len);
        let current = vault.lamports();

        if current <= min_balance {
            // avoid rent violations
            return Err(ProgramError::InsufficientFunds);
        }
        let withdraw_amount = current - min_balance;

        {
            let mut vault_lamports = vault.try_borrow_mut_lamports()?;
            *vault_lamports = vault_lamports
                .checked_sub(withdraw_amount)
                .ok_or(ProgramError::InsufficientFunds)?;
        }

        {
            let mut owner_lamports = owner.try_borrow_mut_lamports()?;
            *owner_lamports = owner_lamports
                .checked_add(withdraw_amount)
                .ok_or(ProgramError::InsufficientFunds)?;
        }

        log!("{} lamports withdrawn from vault", withdraw_amount);
        Ok(())
    }
}

impl<'a> TryFrom<&'a [AccountInfo]> for Withdraw<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        let owner = &accounts[0];
        let vault = &accounts[1];
        Ok(Self { owner, vault })
    }
}
