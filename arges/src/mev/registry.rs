/// Program ID Registry for Solana DeFi Protocols
///
/// This module maintains a registry of known DEX and lending protocol program IDs
/// to supplement instruction-based MEV detection. While instruction analysis is
/// the primary detection method, this registry helps catch edge cases where
/// instruction data alone is insufficient.
///
/// **Maintenance Note**: As new DEXs and protocols launch, add their program IDs here.

use std::collections::HashSet;
use lazy_static::lazy_static;

/// Known Solana DEX program IDs
///
/// Includes major decentralized exchanges and aggregators
pub const DEX_PROGRAMS: &[&str] = &[
    // Jupiter (aggregator)
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",  // Jupiter V6
    "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB",  // Jupiter V4
    "JUP3c2Uh3WA4Ng34oLWNoL4KGBgvJcKQkxrDqEE37Sw",  // Jupiter V3

    // Raydium (AMM)
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",  // Raydium AMM V4
    "5quBtoiQqxF9Jv6KYKctB59NT3gtJD2Y65kdnB1Uev3h",  // Raydium AMM V3
    "27haf8L6oxUeXrHrgEgsexjSY5hbVUWEmvv9Nyxg8vQv",  // Raydium Concentrated Liquidity (CLMM)
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",  // Raydium CPMM

    // Orca (AMM)
    "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP",  // Orca V2
    "DjVE6JNiYqPL2QXyCUUh8rNjHrbz9hXHNYt99MQ59qw1",  // Orca V1
    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",  // Orca Whirlpools (CLMM)

    // Meteora (DLMM, pools)
    "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",  // Meteora DLMM
    "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB",  // Meteora Pools
    "MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky",  // Meteora (general)

    // Phoenix (order book)
    "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY",  // Phoenix V1

    // Lifinity (proactive market maker)
    "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S",  // Lifinity V2
    "2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c",  // Lifinity V1

    // Aldrin (AMM)
    "AMM55ShdkoGRB5jVYPjWziwk8m5MpwyDgsMWHaMSQWH6",  // Aldrin AMM V2
    "CURVGoZn8zycx6FXwwevgBTB2gVvdbGTEpvMJDbgs2t4",  // Aldrin AMM V1

    // Saber (stable swap)
    "SSwpkEEcbUqx4vtoEByFjSkhKdCT862DNVb52nZg1UZ",  // Saber Stable Swap
    "SSwpMgqNDsyV7mAgN9ady4bDVu5ySjmmXejXvy2vLt1",  // Saber Swap

    // Serum (order book DEX)
    "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin",  // Serum V3
    "EUqojwWA2rd19FZrzeBncJsm38Jm1hEhE3zsmX3bRc2o",  // Serum V2

    // Openbook (Serum fork)
    "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX",  // Openbook V1
    "opnb2LAfJYbRMAHHvqjCwQxanZn7ReEHp1k81EohpZb",  // Openbook V2

    // Cropper (yield aggregator with swaps)
    "CTMAxxk34HjKWxQ3QLZK1HpaLXmBveao3ESePXbiyfzh",  // Cropper Finance

    // FluxBeam (AMM)
    "FLUXubRmkEi2q6K3Y9kBPg9248ggaZVsoSFhtJHSrm1X",  // FluxBeam

    // Saros (AMM)
    "SSwapUtytfBdBn1b9NUGG6foMVPtcWgpRU32HToDUZr",  // Saros Swap

    // Crema Finance (CLMM)
    "CLMM9tUoggJu2wagPkkqs9eFG4BWhVBZWkP1qv3Sp7tR",  // Crema CLMM
    "6MLxLqiXaaSUpkgMnWDTuejNZEz3kE7k2woyHGVFw319",  // Crema Finance

    // Invariant (CLMM)
    "HyaB3W9q6XdA5xwpU4XnSZV94htfmbmqJXZcEbRaJutt",  // Invariant

    // GooseFX (order book)
    "GFXsSL5sSaDfNFQUYsHekbWBW1TsFdjDYzACh62tEHxn",  // GooseFX SSL

    // Stepn (DEX)
    "Dooar9JkhdZ7J3LHN3A7YCuoGRUggXhQaG4kijfLGU2j",  // Stepn Swap

    // Marinade Finance (liquid staking with swaps)
    "MarBmsSgKXdrN1egZf5sqe1TMai9K1rChYNDJgjq7aD",  // Marinade

    // Mercurial (stable swap)
    "MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky",  // Mercurial Stable Swap

    // Penguin Finance
    "PSwapMdSai8tjrEXcxFeQth87xC4rRsa4VA5mhGhXkP",  // Penguin Swap

    // Balansol (weighted pools)
    "BALSbAVAu5UKgDVhfWez7rDrJq4ibKxVMRYQ6MhVjVyq",  // Balansol
];

/// Known Solana lending protocol program IDs
///
/// Includes major lending/borrowing platforms that can have liquidations
pub const LENDING_PROGRAMS: &[&str] = &[
    // Solend (lending)
    "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo",  // Solend Main
    "SLENDvALeFN4yFM2J4x2QH1w8s5fRX8VqMKvfTvXJvj",  // Solend V2

    // Mango Markets (perpetuals + lending)
    "mv3ekLzLbnVPNxjSKvqBpU3ZeZXPQdEC3bp5MDEBG68",  // Mango V3
    "4MangoMjqJ2firMokCjjGgoK8d4MXcrgL7XJaL3w6fVg",  // Mango V4

    // Marginfi (lending)
    "MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA",  // Marginfi V2
    "MarBmsSgKXdrN1egZf5sqe1TMai9K1rChYNDJgjq7aD",  // Marginfi V1

    // Kamino Finance (lending + liquidity)
    "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD",  // Kamino Lend
    "6LtLpnUFNByNXLyCoK9wA2MykKAmQNZKBdY8s47dehDc",  // Kamino

    // Port Finance (lending)
    "Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR",  // Port Finance

    // Apricot Finance (lending)
    "6UeJYTLU1adXEMz3SPAkqXV1GRp4R3RUPAsPd8yV3bN8",  // Apricot

    // Larix (lending)
    "LARiXk8x5hXGJmceFfY2UrCMPfGDFpZj5fgJRhkBBBB",  // Larix

    // Francium (leveraged yield farming)
    "FC81tbGt6JWRXidaWYFXxGnTk4VgobhJHATvTRVMqgWj",  // Francium

    // Tulip Protocol (vault + lending)
    "TuLipcqtGVXP9XR62wM8WWCm6a9vhLs7T1uoWBk6FDs",  // Tulip

    // Drift Protocol (perpetuals)
    "dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH",  // Drift V2

    // Jet Protocol (lending)
    "JPLockxtkngHkaQT5AuRYow3HyUv5qWzmhwsCPd653n",  // Jet V2
    "JPPooLEqRo3NCSx82EdE2VZY5vUaSsgskpZPBHNGVLZ",  // Jet V1

    // Hubble Protocol (borrowing)
    "HubbLeXBb7qyLHt3x7gvYaRrxQmmgExb7fCJgDqFuB6T",  // Hubble

    // Oxygen Protocol (prime brokerage)
    "OxygenRQvSW6JjpNEKZvEfFdHCqLgEDfDxJvBiPbP8z",  // Oxygen

    // Cypher Protocol (margin trading)
    "CyphProgE6bSs4KLKBj5yPxFBb7HwB9r8b1zHJsLpNn",  // Cypher
];

lazy_static! {
    /// Set of all known DEX program IDs for O(1) lookup
    pub static ref DEX_PROGRAM_SET: HashSet<String> = {
        DEX_PROGRAMS.iter().map(|s| s.to_string()).collect()
    };

    /// Set of all known lending protocol program IDs for O(1) lookup
    pub static ref LENDING_PROGRAM_SET: HashSet<String> = {
        LENDING_PROGRAMS.iter().map(|s| s.to_string()).collect()
    };
}

/// Program ID Registry
///
/// Provides methods to check if a program ID belongs to a known DEX or lending protocol
pub struct ProgramRegistry;

impl ProgramRegistry {
    /// Check if a program ID is a known DEX
    ///
    /// # Arguments
    /// * `program_id` - The program ID to check (as string)
    ///
    /// # Returns
    /// `true` if the program is a known DEX, `false` otherwise
    pub fn is_dex(program_id: &str) -> bool {
        DEX_PROGRAM_SET.contains(program_id)
    }

    /// Check if a program ID is a known lending protocol
    ///
    /// # Arguments
    /// * `program_id` - The program ID to check (as string)
    ///
    /// # Returns
    /// `true` if the program is a known lending protocol, `false` otherwise
    pub fn is_lending_protocol(program_id: &str) -> bool {
        LENDING_PROGRAM_SET.contains(program_id)
    }

    /// Check if a program ID is any known DeFi protocol
    ///
    /// # Arguments
    /// * `program_id` - The program ID to check (as string)
    ///
    /// # Returns
    /// `true` if the program is a known DEX or lending protocol
    pub fn is_defi_protocol(program_id: &str) -> bool {
        Self::is_dex(program_id) || Self::is_lending_protocol(program_id)
    }

    /// Get all DEX program IDs
    pub fn get_all_dex_programs() -> &'static [&'static str] {
        DEX_PROGRAMS
    }

    /// Get all lending protocol program IDs
    pub fn get_all_lending_programs() -> &'static [&'static str] {
        LENDING_PROGRAMS
    }

    /// Get total count of registered protocols
    pub fn total_registered_protocols() -> usize {
        DEX_PROGRAMS.len() + LENDING_PROGRAMS.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dex_recognition() {
        // Jupiter V6
        assert!(ProgramRegistry::is_dex("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"));

        // Raydium AMM V4
        assert!(ProgramRegistry::is_dex("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"));

        // Orca Whirlpools
        assert!(ProgramRegistry::is_dex("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc"));

        // Not a DEX
        assert!(!ProgramRegistry::is_dex("11111111111111111111111111111111"));
    }

    #[test]
    fn test_lending_recognition() {
        // Solend
        assert!(ProgramRegistry::is_lending_protocol("So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo"));

        // Mango V4
        assert!(ProgramRegistry::is_lending_protocol("4MangoMjqJ2firMokCjjGgoK8d4MXcrgL7XJaL3w6fVg"));

        // Not a lending protocol
        assert!(!ProgramRegistry::is_lending_protocol("11111111111111111111111111111111"));
    }

    #[test]
    fn test_defi_protocol() {
        // DEX
        assert!(ProgramRegistry::is_defi_protocol("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"));

        // Lending
        assert!(ProgramRegistry::is_defi_protocol("So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo"));

        // Neither
        assert!(!ProgramRegistry::is_defi_protocol("11111111111111111111111111111111"));
    }

    #[test]
    fn test_no_duplicates() {
        let all_programs: Vec<_> = DEX_PROGRAMS.iter()
            .chain(LENDING_PROGRAMS.iter())
            .collect();

        let unique_programs: HashSet<_> = all_programs.iter().collect();

        assert_eq!(
            all_programs.len(),
            unique_programs.len(),
            "Registry contains duplicate program IDs"
        );
    }
}
