#!/bin/bash

# Calculate primes up to a given limit using optimized Sieve of Eratosthenes
# Memory-optimized: only tracks odd numbers (saves 50% memory)
# Usage: primes_up_to <limit> [output_format]
#   output_format: "list" (default) or "count"

primes_up_to() {
    local limit=$1
    local output_format="${2:-list}"
    
    # Validate input
    if [[ ! "$limit" =~ ^[0-9]+$ ]]; then
        echo "Error: limit must be a non-negative integer" >&2
        return 1
    fi
    if [[ "$limit" -lt 2 ]]; then
        echo "Error: limit must be >= 2 (no primes below 2)" >&2
        return 1
    fi
    
    # Handle small limits specially
    if [[ "$limit" -lt 3 ]]; then
        if [[ "$output_format" == "count" ]]; then
            echo "1"
        else
            echo "2"
        fi
        return 0
    fi
    
    # Memory-optimized sieve: only track odd numbers
    # Index i represents number (2*i + 3)
    # Array value: 0 = prime, 1 = composite
    # Size: number of odd numbers from 3 to limit
    local sieve_size
    if (( limit % 2 == 0 )); then
        sieve_size=$(( (limit - 2) / 2 ))
    else
        sieve_size=$(( (limit - 1) / 2 ))
    fi
    local -a sieve
    local -a primes
    
    # Initialize sieve (all odd numbers are prime initially)
    local i
    for ((i = 0; i < sieve_size; i++)); do
        sieve[$i]=0
    done
    
    # Sieve: mark composites among odd numbers
    # Use bash integer square root instead of bc for performance
    local limit_sqrt=$limit
    while (( limit_sqrt * limit_sqrt > limit )); do
        ((limit_sqrt--))
    done
    
    local j
    for ((i = 0; i < sieve_size; i++)); do
        if [[ ${sieve[$i]} -eq 0 ]]; then
            local num=$(( 2 * i + 3 ))
            
            # Skip if we've gone past sqrt(limit)
            if [[ $num -gt $limit_sqrt ]]; then
                break
            fi
            
            # Mark multiples of this prime
            # Start from num*num, step by 2*num (to stay on odd multiples)
            local start=$(( (num * num - 3) / 2 ))
            for ((j = start; j < sieve_size; j += num)); do
                sieve[$j]=1
            done
        fi
    done
    
    # Collect primes
    primes+=(2)  # First prime is 2 (we skipped it since we only track odds)
    
    for ((i = 0; i < sieve_size; i++)); do
        if [[ ${sieve[$i]} -eq 0 ]]; then
            primes+=($(( 2 * i + 3 )))
        fi
    done
    
    # Output results
    if [[ "$output_format" == "count" ]]; then
        echo "${#primes[@]}"
    else
        printf "%s\n" "${primes[@]}"
    fi
}

# Alternative: Simple trial division (slower but uses less memory)
# Useful for verification or very small limits
primes_trial_division() {
    local limit=$1
    local output_format="${2:-list}"
    
    if [[ ! "$limit" =~ ^[0-9]+$ ]] || [[ "$limit" -lt 2 ]]; then
        echo "Usage: primes_trial_division <limit> [output_format]" >&2
        return 1
    fi
    
    local -a primes
    
    # Check only odd numbers (except 2)
    primes+=(2)
    
    for ((num = 3; num <= limit; num += 2)); do
        local is_prime=1
        local sqrt_num
        sqrt_num=$(echo "sqrt($num)" | bc)
        
        for ((p = 3; p <= sqrt_num; p += 2)); do
            if ((num % p == 0)); then
                is_prime=0
                break
            fi
        done
        
        if [[ $is_prime -eq 1 ]]; then
            primes+=($num)
        fi
    done
    
    if [[ "$output_format" == "count" ]]; then
        echo "${#primes[@]}"
    else
        printf "%s\n" "${primes[@]}"
    fi
}

# Export functions for sourcing
# Usage: source ./tmp/primes.sh; primes_up_to 10000000 count

# Main entry point when run directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    # Handle --help flag
    if [[ "$1" == "--help" || "$1" == "-h" ]]; then
        echo "Calculate primes up to a given limit using the Sieve of Eratosthenes"
        echo ""
        echo "Usage: $0 <limit> [format]"
        echo "  format: 'list' (default) or 'count'"
        echo ""
        echo "Examples:"
        echo "  $0 100 list   # list all primes up to 100"
        echo "  $0 100 count  # count primes up to 100"
        echo "  $0 100        # list primes up to 100 (default)"
        echo "  $0 10_000_000 # underscore separators are allowed"
        echo ""
        echo "Note: For pure Bash, practical limit is ~1,000,000"
        exit 0
    fi
    
    if [[ $# -lt 1 ]]; then
        echo "Usage: $0 <limit> [format]" >&2
        echo "  format: 'list' (default) or 'count'" >&2
        echo "" >&2
        echo "Examples:"
        echo "  $0 100 list   # list all primes up to 100"
        echo "  $0 100 count  # count primes up to 100"
        exit 1
    fi
    
    # Strip underscore separators for readability (e.g., 10_000_000)
    limit="${1//_/}"
    
    if ! [[ "$limit" =~ ^[0-9]+$ ]]; then
        echo "Error: limit must be a non-negative integer (e.g., 100)" >&2
        exit 1
    fi
    
    if [[ "$limit" -lt 2 ]]; then
        echo "Error: limit must be >= 2 (no primes below 2)" >&2
        exit 1
    fi
    
    # Prevent integer overflow or long hangs (Bash is slow for large sieves)
    if [[ "$limit" -gt 1000000 ]]; then
        echo "Error: limit too large (max: 1000000 for pure Bash)" >&2
        echo "  For larger limits, consider using awk or Python instead" >&2
        exit 1
    fi
    
    format="${2:-list}"
    if [[ "$format" != "list" && "$format" != "count" ]]; then
        echo "Error: format must be 'list' or 'count'" >&2
        echo "  Usage: $0 <limit> [list|count]" >&2
        exit 1
    fi
    
    primes_up_to "$limit" "$format"
fi
