use anchor_lang::prelude::*;

declare_id!("3zcbbgzgNqWLyTUJKzuzzT9t1t3VpdcuSQw9vA5qj3Zm");

#[program]
pub mod kontrakt {
    use super::*;

    // Pojedynczy alarm krytyczny — wysyłany natychmiast po wykryciu
    pub fn log_event(ctx: Context<LogEvent>, message: String) -> Result<()> {
        let event_account = &mut ctx.accounts.event_account;
        event_account.message = message.clone();
        event_account.timestamp = Clock::get()?.unix_timestamp;

        msg!("Zapisano alarm: {}", message);
        Ok(())
    }

    // Paczka pełnej historii — wysyłana co interwał czasowy
    // Zawiera CID z IPFS + hash SHA-256 do weryfikacji integralności
    pub fn log_batch(
        ctx: Context<LogBatch>,
        cid: String,
        content_hash: String,
        records_count: u32,
    ) -> Result<()> {
        let batch_account = &mut ctx.accounts.batch_account;
        batch_account.cid = cid.clone();
        batch_account.content_hash = content_hash;
        batch_account.records_count = records_count;
        batch_account.timestamp = Clock::get()?.unix_timestamp;

        msg!("Zapisano paczkę: CID={} rekordów={}", cid, records_count);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct LogEvent<'info> {
    #[account(init, payer = user, space = 8 + 300)]
    pub event_account: Account<'info, EventData>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct LogBatch<'info> {
    // 8 (dyskryminator) + 100 (CID) + 80 (hash) + 4 (count) + 8 (timestamp) + zapas
    #[account(init, payer = user, space = 8 + 100 + 80 + 4 + 8 + 100)]
    pub batch_account: Account<'info, BatchData>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[account]
pub struct EventData {
    pub message: String,
    pub timestamp: i64,
}

#[account]
pub struct BatchData {
    pub cid: String,           // CID IPFS, np. "QmXyz..."
    pub content_hash: String,  // SHA-256 zaszyfrowanej paczki (hex)
    pub records_count: u32,    // ile odczytów było w paczce
    pub timestamp: i64,        // kiedy paczka została zarchiwizowana
}