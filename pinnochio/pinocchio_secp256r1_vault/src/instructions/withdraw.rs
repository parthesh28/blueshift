use pinocchio::{
    account_info::{AccountInfo, Ref},
    instruction::{Seed, Signer},
    program_error::ProgramError,
    sysvars::{
        clock::Clock,
        instructions::{Instructions, IntrospectedInstruction},
        Sysvar,
    },
    ProgramResult,
};
use pinocchio_secp256r1_instruction::{Secp256r1Instruction, Secp256r1Pubkey};
use pinocchio_system::instructions::Transfer;

//structs
pub struct WithdrawAccounts<'a> {
    pub owner: &'a AccountInfo,
    pub vault: &'a AccountInfo,
    pub instructions: &'a AccountInfo,
}

pub struct WithdrawInstructionData {
    pub bump: [u8; 1],
}

pub struct Withdraw<'a> {
    pub accounts: WithdrawAccounts<'a>,
    pub instruction_datas: WithdrawInstructionData,
}

//validation
impl<'a> TryFrom<&'a [AccountInfo]> for WithdrawAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountInfo]) -> Result<Self, Self::Error> {
        let [owner, vault, instructions, _system_program] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        if !owner.is_signer() {
            return Err(ProgramError::InvalidAccountOwner);
        };

        if !vault.is_owned_by(&pinocchio_system::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        };

        if vault.lamports().eq(&0) {
            return Err(ProgramError::AccountDataTooSmall);
        }

        Ok(Self {
            owner,
            vault,
            instructions,
        })
    }
}



impl<'a> TryFrom<&'a [u8]> for WithdrawInstructionData {
    type Error = ProgramError;

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        let ix: Self = unsafe {
            core::mem::transmute(
                TryInto::<[u8; size_of::<WithdrawInstructionData>()]>::try_into(value)
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
            )
        };

        Ok(ix)
    }
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountInfo])> for Withdraw<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a [AccountInfo])) -> Result<Self, Self::Error> {
        let accounts = WithdrawAccounts::try_from(accounts)?;
        let instruction_datas = WithdrawInstructionData::try_from(data)?;

        Ok(Self {
            accounts,
            instruction_datas,
        })
    }
}

//withdraw ix
impl<'a> Withdraw<'a> {
    pub const DISCRIMINATOR: &'a u8 = &1;

    pub fn process(&self) -> ProgramResult {
        let instructions: Instructions<Ref<[u8]>> =
            Instructions::try_from(self.accounts.instructions)?;

        let ix: IntrospectedInstruction = instructions.get_instruction_relative(1)?;

        let secp256r1_ix = Secp256r1Instruction::try_from(&ix)?;

        let signer: Secp256r1Pubkey = *secp256r1_ix.get_signer(0)?;

        if secp256r1_ix.num_signatures() != 1 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let (payer, expiry) = secp256r1_ix
            .get_message_data(0)?
            .split_at_checked(32)
            .ok_or(ProgramError::InvalidInstructionData)?;

        if self.accounts.owner.key().ne(&payer) {
            return Err(ProgramError::InvalidInstructionData);
        }

        let current = Clock::get()?.unix_timestamp;

        let expiry = i64::from_le_bytes(
            expiry
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );

        if current > expiry {
            return Err(ProgramError::InvalidInstructionData);
        }

        let vault_seeds = [
            Seed::from(b"vault"),
            Seed::from(signer[..1].as_ref()),
            Seed::from(signer[1..].as_ref()),
            Seed::from(&self.instruction_datas.bump),
        ];

        let vault_signer = Signer::from(&vault_seeds);
         Transfer {
            from: self.accounts.vault,
            to: self.accounts.owner,
            lamports: self.accounts.vault.lamports(),
        }
        .invoke_signed(&[vault_signer])

    }
}