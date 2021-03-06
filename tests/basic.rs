extern crate solana_program;
extern crate solana_sdk;
use borsh::BorshDeserialize;

use solana_client::rpc_client::RpcClient;

use solana_program::{
    instruction::Instruction, message::Message, native_token::LAMPORTS_PER_SOL, program_pack::Pack,
    pubkey::Pubkey,
};
use solana_sdk::account::ReadableAccount;
use solana_sdk::{
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use spl_token::{
    instruction::mint_to_checked,
    state::{Account, Mint},
};
use std::{collections::BTreeMap, thread::sleep, time::Duration};
use tokenitis::sdk::InstructionBuilder;
use tokenitis::state::{Token, TransformMetadata};
use tokenitis::state::{Tokenitis, Transform};
use tokenitis::tokenitis_instruction::create_transform::CreateTransformArgs;
use tokenitis::tokenitis_instruction::execute_transform::{Direction, ExecuteTransformArgs};

const TRANSFORM_AMOUNT: u64 = 50;
const FEE_PERCENT: u64 = 5;
const OUTPUT_PROGRAM_ACC_SUPPLY: u64 = 1000;
const INPUT_CALLER_ACC_SUPPLY: u64 = 1000;

#[test]
fn basic() -> Result<(), Box<dyn std::error::Error>> {
    let user_keypair = Keypair::new();
    let user = &user_keypair.pubkey();
    let client: RpcClient = RpcClient::new("http://localhost:8899".to_string());

    // Create account that will test the functionality of the smart contract
    let sig = client.request_airdrop(user, LAMPORTS_PER_SOL * 10)?;
    confirm_transactions(&client, vec![sig])?;
    let user_balance = client.get_balance(user)?;
    println!(
        "user account created with pubkey - {}, with balance - {}\n",
        user_keypair.pubkey(),
        user_balance
    );

    // Create input mints
    let mut instructions: Vec<Instruction> = Vec::new();
    let input_mint1 = Keypair::new();
    let input_mint2 = Keypair::new();
    let mint_rent = client.get_minimum_balance_for_rent_exemption(Mint::LEN)?;
    InstructionBuilder::create_spl_token_mint(&input_mint1.pubkey(), user, None, 0, mint_rent)?
        .iter()
        .for_each(|i| instructions.push(i.clone()));
    InstructionBuilder::create_spl_token_mint(&input_mint2.pubkey(), user, None, 0, mint_rent)?
        .iter()
        .for_each(|i| instructions.push(i.clone()));
    let sig = create_and_send_tx(
        &client,
        instructions,
        vec![&user_keypair, &input_mint1, &input_mint2],
        Some(user),
    )?;
    confirm_transactions(&client, vec![sig])?;
    println!(
        "created input mints, input1 - {}, input2 - {}\n",
        input_mint1.pubkey(),
        input_mint2.pubkey()
    );

    // Initialize tokenitis
    let output_mint1 = Keypair::new();
    let output_mint2 = Keypair::new();
    let input1_program_account = Keypair::new();
    let input2_program_account = Keypair::new();
    let output1_program_account = Keypair::new();
    let output2_program_account = Keypair::new();

    let mut inputs: BTreeMap<Pubkey, Token> = BTreeMap::new();
    let mut outputs: BTreeMap<Pubkey, Token> = BTreeMap::new();
    let mut output_supply: BTreeMap<Pubkey, u64> = BTreeMap::new();
    inputs.insert(
        input_mint1.pubkey(),
        Token {
            account: input1_program_account.pubkey(),
            amount: TRANSFORM_AMOUNT,
        },
    );
    inputs.insert(
        input_mint2.pubkey(),
        Token {
            account: input2_program_account.pubkey(),
            amount: TRANSFORM_AMOUNT,
        },
    );
    outputs.insert(
        output_mint1.pubkey(),
        Token {
            account: output1_program_account.pubkey(),
            amount: TRANSFORM_AMOUNT,
        },
    );
    outputs.insert(
        output_mint2.pubkey(),
        Token {
            account: output2_program_account.pubkey(),
            amount: TRANSFORM_AMOUNT,
        },
    );
    output_supply.insert(output_mint1.pubkey(), OUTPUT_PROGRAM_ACC_SUPPLY);
    output_supply.insert(output_mint2.pubkey(), OUTPUT_PROGRAM_ACC_SUPPLY);
    let args = CreateTransformArgs {
        metadata: TransformMetadata {
            name: "test123".to_string(),
            image: "".to_string(),
        },
        fee: Some(FEE_PERCENT),
        inputs,
        outputs,
    };
    let spl_token_rent =
        client.get_minimum_balance_for_rent_exemption(spl_token::state::Account::LEN)?;
    let spl_mint_rent =
        client.get_minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN)?;

    let instructions =
        InstructionBuilder::create_transform_input_accounts(user, spl_token_rent, args.clone())?;
    let sig1 = create_and_send_tx(
        &client,
        instructions,
        vec![
            &user_keypair,
            &input1_program_account,
            &input2_program_account,
        ],
        Some(user),
    )?;

    let instructions = InstructionBuilder::create_trarnsform_output_accounts(
        user,
        spl_token_rent,
        spl_mint_rent,
        args.clone(),
        output_supply,
    )?;
    let sig2 = create_and_send_tx(
        &client,
        instructions,
        vec![
            &user_keypair,
            &output_mint1,
            &output_mint2,
            &output1_program_account,
            &output2_program_account,
        ],
        Some(user),
    )?;

    let instructions = InstructionBuilder::create_transform_fee_accounts(user, user, args.clone())?;
    let sig3 = create_and_send_tx(&client, instructions, vec![&user_keypair], Some(user))?;
    confirm_transactions(&client, vec![sig1, sig2, sig3])?;

    println!("created program accounts");

    let (tokenitis_pub, _) = tokenitis::state::Tokenitis::find_tokenitis_address(&tokenitis::id());
    let tokenitis_account = client.get_account(&tokenitis_pub)?;
    let tokenitis_state = Tokenitis::try_from_slice(tokenitis_account.data())
        .unwrap_or(Tokenitis { num_transforms: 0 });

    let instructions = InstructionBuilder::create_transform(
        tokenitis::id(),
        user,
        tokenitis_state.num_transforms + 1,
        args.clone(),
    )?;
    let sig = create_and_send_tx(&client, instructions, vec![&user_keypair], Some(user))?;
    confirm_transactions(&client, vec![sig])?;
    println!("initialized tokenitis - args - {:?}\n", args);

    // Create user token accounts
    let mut instructions: Vec<Instruction> = Vec::new();
    let mut user_inputs: BTreeMap<Pubkey, Pubkey> = BTreeMap::new();
    let mut user_outputs: BTreeMap<Pubkey, Pubkey> = BTreeMap::new();
    let input1_user_account = Keypair::new();
    let input2_user_account = Keypair::new();
    let output1_user_account = Keypair::new();
    let output2_user_account = Keypair::new();
    user_inputs.insert(input_mint1.pubkey(), input1_user_account.pubkey());
    user_inputs.insert(input_mint2.pubkey(), input2_user_account.pubkey());
    user_outputs.insert(output_mint1.pubkey(), output1_user_account.pubkey());
    user_outputs.insert(output_mint2.pubkey(), output2_user_account.pubkey());
    let account_rent = client.get_minimum_balance_for_rent_exemption(Account::LEN)?;
    for (mint, user_account) in user_inputs.iter() {
        InstructionBuilder::create_spl_token_account(mint, user_account, user, account_rent)?
            .iter()
            .for_each(|i| instructions.push(i.clone()));
        instructions.push(mint_to_checked(
            &spl_token::ID,
            mint,
            user_account,
            user,
            &[user],
            INPUT_CALLER_ACC_SUPPLY,
            0,
        )?)
    }
    for (mint, user_account) in user_outputs.iter() {
        InstructionBuilder::create_spl_token_account(mint, user_account, user, account_rent)?
            .iter()
            .for_each(|i| instructions.push(i.clone()));
    }
    let sig = create_and_send_tx(
        &client,
        instructions,
        vec![
            &user_keypair,
            &input1_user_account,
            &input2_user_account,
            &output1_user_account,
            &output2_user_account,
        ],
        Some(user),
    )?;
    confirm_transactions(&client, vec![sig])?;
    println!(
        "created user accounts, inputs - {:?}, outputs - {:?}\n",
        user_inputs.clone(),
        user_outputs.clone()
    );

    assert_eq!(
        client
            .get_token_account_balance(&input1_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        INPUT_CALLER_ACC_SUPPLY
    );
    assert_eq!(
        client
            .get_token_account_balance(&input2_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        INPUT_CALLER_ACC_SUPPLY
    );
    assert_eq!(
        client
            .get_token_account_balance(&output1_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&output2_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&input1_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&input2_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&output1_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        OUTPUT_PROGRAM_ACC_SUPPLY,
    );
    assert_eq!(
        client
            .get_token_account_balance(&output2_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        OUTPUT_PROGRAM_ACC_SUPPLY,
    );

    let (transform_pub, _) = tokenitis::state::Tokenitis::find_transform_address(
        &tokenitis::id(),
        tokenitis_state.num_transforms + 1,
    );
    let transform_account = client.get_account(&transform_pub)?;
    let transform_state = Transform::try_from_slice(transform_account.data())?;

    // Execute tokenitis forward
    let args = ExecuteTransformArgs {
        direction: Direction::Forward,
    };
    let instructions = InstructionBuilder::execute_transform(
        tokenitis::id(),
        user,
        transform_state.clone(),
        args.clone(),
        user_inputs.clone(),
        user_outputs.clone(),
    )?;
    let sig = create_and_send_tx(&client, instructions, vec![&user_keypair], Some(user))?;
    confirm_transactions(&client, vec![sig])?;

    println!(
        "successfully executed tokenitis forward, args - {:?}\n",
        args,
    );

    let fee = tokenitis::util::calculate_fee(TRANSFORM_AMOUNT, FEE_PERCENT);
    let fee_account1 =
        spl_associated_token_account::get_associated_token_address(user, &input_mint1.pubkey());
    let fee_account2 =
        spl_associated_token_account::get_associated_token_address(user, &input_mint2.pubkey());

    println!(
        "fee - {}, fee account1 - {:?}, fee account2 - {:?}\n",
        fee, fee_account1, fee_account2
    );

    assert_eq!(
        client
            .get_token_account_balance(&fee_account1)?
            .amount
            .parse::<u64>()?,
        fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&fee_account2)?
            .amount
            .parse::<u64>()?,
        fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&input1_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        INPUT_CALLER_ACC_SUPPLY - TRANSFORM_AMOUNT - fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&input2_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        INPUT_CALLER_ACC_SUPPLY - TRANSFORM_AMOUNT - fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&output1_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        TRANSFORM_AMOUNT
    );
    assert_eq!(
        client
            .get_token_account_balance(&output2_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        TRANSFORM_AMOUNT
    );
    assert_eq!(
        client
            .get_token_account_balance(&input1_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        TRANSFORM_AMOUNT
    );
    assert_eq!(
        client
            .get_token_account_balance(&input2_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        TRANSFORM_AMOUNT,
    );
    assert_eq!(
        client
            .get_token_account_balance(&output1_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        OUTPUT_PROGRAM_ACC_SUPPLY - TRANSFORM_AMOUNT,
    );
    assert_eq!(
        client
            .get_token_account_balance(&output2_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        OUTPUT_PROGRAM_ACC_SUPPLY - TRANSFORM_AMOUNT,
    );

    // Execute tokenitis reverse
    let args = ExecuteTransformArgs {
        direction: Direction::Reverse,
    };
    let instructions = InstructionBuilder::execute_transform(
        tokenitis::id(),
        user,
        transform_state,
        args.clone(),
        user_inputs,
        user_outputs,
    )?;
    let sig = create_and_send_tx(&client, instructions, vec![&user_keypair], Some(user))?;
    confirm_transactions(&client, vec![sig])?;

    println!(
        "successfully executed tokenitis reverse, args - {:?}\n",
        args,
    );

    assert_eq!(
        client
            .get_token_account_balance(&fee_account1)?
            .amount
            .parse::<u64>()?,
        fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&fee_account2)?
            .amount
            .parse::<u64>()?,
        fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&input1_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        INPUT_CALLER_ACC_SUPPLY - fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&input2_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        INPUT_CALLER_ACC_SUPPLY - fee,
    );
    assert_eq!(
        client
            .get_token_account_balance(&output1_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&output2_user_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&input1_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&input2_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        0
    );
    assert_eq!(
        client
            .get_token_account_balance(&output1_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        OUTPUT_PROGRAM_ACC_SUPPLY,
    );
    assert_eq!(
        client
            .get_token_account_balance(&output2_program_account.pubkey())?
            .amount
            .parse::<u64>()?,
        OUTPUT_PROGRAM_ACC_SUPPLY,
    );

    Ok(())
}

fn create_and_send_tx(
    client: &RpcClient,
    instructions: Vec<Instruction>,
    signers: Vec<&dyn Signer>,
    payer: Option<&Pubkey>,
) -> Result<Signature, Box<dyn std::error::Error>> {
    let msg = Message::new(instructions.as_slice(), payer);
    let tx = Transaction::new(&signers, msg, client.get_latest_blockhash()?);
    let sig = client.send_transaction(&tx)?;

    Ok(sig)
}

fn confirm_transactions(
    client: &RpcClient,
    sigs: Vec<Signature>,
) -> Result<(), Box<dyn std::error::Error>> {
    for sig in sigs {
        loop {
            if client.confirm_transaction(&sig)? {
                break;
            }
            sleep(Duration::from_millis(500))
        }
    }

    Ok(())
}
