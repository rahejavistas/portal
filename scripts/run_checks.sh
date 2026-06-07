#!/usr/bin/env bash
# Vistas File Server Local Checks Runner
# Runs formatting, compilation checks, and strict Clippy static analysis.

set -euo pipefail

# Colorful printing helpers
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Vistas File Server - Code Quality Check ===${NC}\n"

# Prioritize ~/.cargo/bin/cargo if it exists to avoid older system cargo version issues
CARGO_BIN="cargo"
if [ -f "$HOME/.cargo/bin/cargo" ]; then
    CARGO_BIN="$HOME/.cargo/bin/cargo"
fi

# Verify cargo is installed
if ! command -v "$CARGO_BIN" &> /dev/null; then
    echo -e "${RED}❌ Error: cargo is not installed or not found. Please install Rust and try again.${NC}"
    exit 1
fi

echo -e "${BLUE}Using cargo binary: $CARGO_BIN${NC}\n"

# Step 1: Auto-format, then verify
echo -e "${YELLOW}🔍 Step 1/3: Formatting code (cargo fmt)...${NC}"
"$CARGO_BIN" fmt --all
if ! "$CARGO_BIN" fmt --all -- --check; then
    echo -e "\n${RED}❌ Formatting could not be fully applied!${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Code formatted and verified!${NC}\n"

# Step 2: Cargo Check
echo -e "${YELLOW}🔍 Step 2/3: Checking compilation and type safety (cargo check)...${NC}"
if ! "$CARGO_BIN" check --all-targets; then
    echo -e "\n${RED}❌ Compilation check failed!${NC}"
    echo -e "${YELLOW}💡 Tip: Review the compiler errors above and fix type/reference issues.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Type checks passed successfully!${NC}\n"

# Step 3: Cargo Clippy
echo -e "${YELLOW}🔍 Step 3/3: Running static code analysis and strict lints (cargo clippy)...${NC}"
if ! "$CARGO_BIN" clippy --all-targets -- -D warnings; then
    echo -e "\n${RED}❌ Clippy static lint analysis failed!${NC}"
    echo -e "${YELLOW}💡 Tip: Fix any clippy warnings or add #[allow(...)] tags where appropriate.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Clippy code analysis passed! (0 warnings, 0 errors)${NC}\n"

echo -e "${GREEN}🎉 CONGRATULATIONS! All code quality checks passed perfectly! Your code is 100% ready to commit.${NC}"
exit 0
