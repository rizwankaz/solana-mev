use std::collections::HashMap;

/// Known token metadata
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub symbol: String,
    pub mint: String,
    pub decimals: u8,
}

/// Registry of known Solana tokens
pub struct TokenRegistry {
    tokens: HashMap<String, TokenInfo>,
}

impl TokenRegistry {
    pub fn new() -> Self {
        let mut tokens = HashMap::new();

        // SOL (wrapped)
        tokens.insert(
            "So11111111111111111111111111111111111111112".to_string(),
            TokenInfo {
                symbol: "SOL".to_string(),
                mint: "So11111111111111111111111111111111111111112".to_string(),
                decimals: 9,
            },
        );

        // USDC
        tokens.insert(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            TokenInfo {
                symbol: "USDC".to_string(),
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                decimals: 6,
            },
        );

        // USDT
        tokens.insert(
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string(),
            TokenInfo {
                symbol: "USDT".to_string(),
                mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string(),
                decimals: 6,
            },
        );

        // RAY
        tokens.insert(
            "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R".to_string(),
            TokenInfo {
                symbol: "RAY".to_string(),
                mint: "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R".to_string(),
                decimals: 6,
            },
        );

        // POPCAT
        tokens.insert(
            "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr".to_string(),
            TokenInfo {
                symbol: "POPCAT".to_string(),
                mint: "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr".to_string(),
                decimals: 9,
            },
        );

        // ZBCN
        tokens.insert(
            "ZBCNpuD7YMXzTHB2fhGkGi78MNsHGLRXUhRewNRm9RU".to_string(),
            TokenInfo {
                symbol: "ZBCN".to_string(),
                mint: "ZBCNpuD7YMXzTHB2fhGkGi78MNsHGLRXUhRewNRm9RU".to_string(),
                decimals: 6,
            },
        );

        Self { tokens }
    }

    pub fn get_symbol(&self, mint: &str) -> Option<&str> {
        self.tokens.get(mint).map(|info| info.symbol.as_str())
    }

    pub fn get_decimals(&self, mint: &str) -> Option<u8> {
        self.tokens.get(mint).map(|info| info.decimals)
    }

    pub fn is_known(&self, mint: &str) -> bool {
        self.tokens.contains_key(mint)
    }
}

impl Default for TokenRegistry {
    fn default() -> Self {
        Self::new()
    }
}
