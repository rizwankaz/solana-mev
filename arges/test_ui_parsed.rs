// Quick test to see UiParsedInstruction structure
use solana_transaction_status::UiParsedInstruction;

fn main() {
    // This will show the fields available
    let example = UiParsedInstruction {
        program: "test".to_string(),
        program_id: "test_id".to_string(),
        parsed: serde_json::json!({}),
    };
    println!("{:?}", example);
}
