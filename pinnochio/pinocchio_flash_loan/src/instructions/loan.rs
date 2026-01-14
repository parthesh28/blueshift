use pinocchio::{account_info::AccountInfo, instruction::{Seed, Signer}, msg, program_error::ProgramError, sysvars::{instructions::{Instructions, INSTRUCTIONS_ID}, rent::Rent, Sysvar}, ProgramResult};
use pinocchio_system::instructions::CreateAccount;
use pinocchio_token::instructions::Transfer;

use crate::{get_token_amount, LoanData, Repay, ID};

pub struct LoanAccounts<'a> {
    pub borrower: &'a AccountInfo,
    pub protocol: &'a AccountInfo,
    pub loan: &'a AccountInfo,
    pub instruction_sysvar: &'a AccountInfo,
    pub token_accounts: &'a [AccountInfo],
}
 
impl<'a> TryFrom<&'a [AccountInfo]> for LoanAccounts<'a> {
    type Error = ProgramError;
 
    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let [borrower, protocol, loan, instruction_sysvar, _token_program, _system_program, token_accounts @ ..] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };
 
        if instruction_sysvar.key() != &INSTRUCTIONS_ID {
            return Err(ProgramError::UnsupportedSysvar);
        }
 
        if (token_accounts.len() % 2).ne(&0) || token_accounts.len().eq(&0) {
            return Err(ProgramError::InvalidAccountData);
        }
 
        if loan.try_borrow_data()?.len().ne(&0) {
            return Err(ProgramError::InvalidAccountData);
        }
 
        Ok(Self {
            borrower,
            protocol,
            loan,
            instruction_sysvar,
            token_accounts,
        })
    }
}

pub struct LoanInstructionData<'a> {
    pub bump: [u8; 1],
    pub fee: u16,
    pub amounts: &'a [u64],
}
 
impl<'a> TryFrom<&'a [u8]> for LoanInstructionData<'a> {
    type Error = ProgramError;
 
    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        let (bump, data) = data.split_first().ok_or(ProgramError::InvalidInstructionData)?;
 
        let (fee, data) = data.split_at_checked(size_of::<u16>()).ok_or(ProgramError::InvalidInstructionData)?;
 
        if data.len() % size_of::<u64>() != 0 {
            return Err(ProgramError::InvalidInstructionData);
        }
 
        let amounts: &[u64] = unsafe {
            core::slice::from_raw_parts(
                data.as_ptr() as *const u64,
                data.len() / size_of::<u64>()
            )
        };
 
        Ok(Self { bump: [*bump], fee: u16::from_le_bytes(fee.try_into().map_err(|_| ProgramError::InvalidInstructionData)?), amounts })
    }
}
pub struct Loan<'a> {
    pub accounts: LoanAccounts<'a>,
    pub instruction_data: LoanInstructionData<'a>,
}
 
impl<'a> TryFrom<(&'a [u8], &'a [AccountInfo])> for Loan<'a> {
    type Error = ProgramError;
 
    fn try_from((data, accounts): (&'a [u8], &'a [AccountInfo])) -> Result<Self, Self::Error> {
        let accounts = LoanAccounts::try_from(accounts)?;
        let instruction_data = LoanInstructionData::try_from(data)?;
 
        if instruction_data.amounts.len() != accounts.token_accounts.len() / 2 {
            return Err(ProgramError::InvalidInstructionData);
        }
 
        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Loan<'a> {
    pub const DISCRIMINATOR: &'a u8 = &0;
 
    pub fn process(&mut self) -> ProgramResult {
        let fee = self.instruction_data.fee.to_le_bytes();
 
        let signer_seeds = [
            Seed::from("protocol".as_bytes()),
            Seed::from(&fee),
            Seed::from(&self.instruction_data.bump),
        ];
        let signer_seeds = [Signer::from(&signer_seeds)];
 
        let size = size_of::<LoanData>() * self.instruction_data.amounts.len();
        let lamports = Rent::get()?.minimum_balance(size);
 
        CreateAccount {
            from: self.accounts.borrower,
            to: self.accounts.loan,
            lamports,
            space: size as u64,
            owner: &ID,
        }.invoke()?;
 
        let mut loan_data = self.accounts.loan.try_borrow_mut_data()?;
        let loan_entries = unsafe {
            core::slice::from_raw_parts_mut(
                loan_data.as_mut_ptr() as *mut LoanData,
                self.instruction_data.amounts.len()
            )
        };

        for (i, amount) in self.instruction_data.amounts.iter().enumerate() {
            let protocol_token_account = &self.accounts.token_accounts[i * 2];
            let borrower_token_account = &self.accounts.token_accounts[i * 2 + 1];
        
            let balance = get_token_amount(&protocol_token_account.try_borrow_data()?);
            let balance_with_fee = balance.checked_add(
                amount.checked_mul(self.instruction_data.fee as u64)
                    .and_then(|x| x.checked_div(10_000))
                    .ok_or(ProgramError::InvalidInstructionData)?
            ).ok_or(ProgramError::InvalidInstructionData)?;
        
            loan_entries[i] = LoanData {
                protocol_token_account: *protocol_token_account.key(),
                balance: balance_with_fee,
            };
        
            Transfer {
                from: protocol_token_account,
                to: borrower_token_account,
                authority: self.accounts.protocol,
                amount: *amount,
            }.invoke_signed(&signer_seeds)?;
        }

        let instruction_sysvar = unsafe { Instructions::new_unchecked(self.accounts.instruction_sysvar.try_borrow_data()?) };
        let num_instructions = instruction_sysvar.num_instructions();
        let instruction = instruction_sysvar.load_instruction_at(num_instructions as usize - 1)?;
        
        if instruction.get_program_id() != &crate::ID {
            return Err(ProgramError::InvalidInstructionData);
        }
        
        if unsafe { *(instruction.get_instruction_data().as_ptr()) } != *Repay::DISCRIMINATOR {
            return Err(ProgramError::InvalidInstructionData);
        }
        
        if unsafe { instruction.get_account_meta_at_unchecked(1).key } != *self.accounts.loan.key() {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(())
    }
}