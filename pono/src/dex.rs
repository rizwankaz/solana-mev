use std::collections::HashMap;

/// Known DEX programs on Solana
pub struct DexRegistry {
    programs: HashMap<String, DexInfo>,
}

#[derive(Debug, Clone)]
pub struct DexInfo {
    pub name: String,
    pub program_id: String,
}

impl DexRegistry {
    pub fn new() -> Self {
        let mut programs = HashMap::new();

        // Raydium programs
        programs.insert(
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8".to_string(),
            DexInfo {
                name: "Raydium V4".to_string(),
                program_id: "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8".to_string(),
            },
        );
        programs.insert(
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK".to_string(),
            DexInfo {
                name: "Raydium CLMM".to_string(),
                program_id: "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK".to_string(),
            },
        );

        // Orca programs
        programs.insert(
            "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc".to_string(),
            DexInfo {
                name: "Orca Whirlpool".to_string(),
                program_id: "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc".to_string(),
            },
        );
        programs.insert(
            "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP".to_string(),
            DexInfo {
                name: "Orca V1".to_string(),
                program_id: "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP".to_string(),
            },
        );

        // Meteora
        programs.insert(
            "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo".to_string(),
            DexInfo {
                name: "Meteora DLMM".to_string(),
                program_id: "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo".to_string(),
            },
        );

        // Phoenix
        programs.insert(
            "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY".to_string(),
            DexInfo {
                name: "Phoenix".to_string(),
                program_id: "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY".to_string(),
            },
        );

        // Openbook
        programs.insert(
            "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX".to_string(),
            DexInfo {
                name: "Openbook V1".to_string(),
                program_id: "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX".to_string(),
            },
        );
        programs.insert(
            "opnb2LAfJYbRMAHHvqjCwQxanZn7ReEHp1k81EohpZb".to_string(),
            DexInfo {
                name: "Openbook V2".to_string(),
                program_id: "opnb2LAfJYbRMAHHvqjCwQxanZn7ReEHp1k81EohpZb".to_string(),
            },
        );

        // Jupiter Aggregator
        programs.insert(
            "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".to_string(),
            DexInfo {
                name: "Jupiter V6".to_string(),
                program_id: "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".to_string(),
            },
        );

        // PancakeSwap
        programs.insert(
            "HpNfyc2Saw7RKkQd8nEL4khUcuPhQ7WwY1B2qjx8jxFq".to_string(),
            DexInfo {
                name: "PancakeSwap".to_string(),
                program_id: "HpNfyc2Saw7RKkQd8nEL4khUcuPhQ7WwY1B2qjx8jxFq".to_string(),
            },
        );

        // Add more DEXs as needed
        programs.insert(
            "9GCBb4NsSDRuEkkGfQkN37oppmMVPops5sXDWhAKAvQQ".to_string(),
            DexInfo {
                name: "GooseFX".to_string(),
                program_id: "9GCBb4NsSDRuEkkGfQkN37oppmMVPops5sXDWhAKAvQQ".to_string(),
            },
        );

        programs.insert(
            "CroWg74XNDF8UMnAZVbXx49iVj7iJ7b4CsqTCVWF7aK".to_string(),
            DexInfo {
                name: "Cropper".to_string(),
                program_id: "CroWg74XNDF8UMnAZVbXx49iVj7iJ7b4CsqTCVWF7aK".to_string(),
            },
        );

        Self { programs }
    }

    pub fn get_dex_name(&self, program_id: &str) -> Option<&str> {
        self.programs.get(program_id).map(|info| info.name.as_str())
    }

    pub fn is_dex(&self, program_id: &str) -> bool {
        self.programs.contains_key(program_id)
    }
}

impl Default for DexRegistry {
    fn default() -> Self {
        Self::new()
    }
}
