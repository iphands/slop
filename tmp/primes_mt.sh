#!/bin/bash

# Parallel Sieve of Eratosthenes for pure Bash
# Uses segmented approach with background workers
# Usage: primes_parallel <limit> [format] [--parallel [workers]]
#   format: 'list' (default) or 'count'
#   workers: number of parallel workers (default: auto-detect)

# Generate base primes up to sqrt(limit) using single-threaded sieve
generate_base_primes() {
    local limit=$1
    local output_file=$2
    
    # Handle small limits
    if [[ "$limit" -lt 3 ]]; then
        echo "2" > "$output_file"
        return 0
    fi
    
    # Only track odd numbers
    local sieve_size=$(( (limit - 1) / 2 ))
    local -a sieve
    
    for ((i = 0; i < sieve_size; i++)); do
        sieve[$i]=0
    done
    
    # Calculate sqrt using bash integer arithmetic
    local limit_sqrt=$limit
    while (( limit_sqrt * limit_sqrt > limit )); do
        ((limit_sqrt--))
    done
    
    for ((i = 0; i < sieve_size; i++)); do
        if [[ ${sieve[$i]} -eq 0 ]]; then
            local num=$(( 2 * i + 3 ))
            if [[ $num -gt $limit_sqrt ]]; then
                break
            fi
            local start=$(( (num * num - 3) / 2 ))
            for ((j = start; j < sieve_size; j += num)); do
                sieve[$j]=1
            done
        fi
    done
    
    # Write to file
    echo "2" > "$output_file"
    for ((i = 0; i < sieve_size; i++)); do
        if [[ ${sieve[$i]} -eq 0 ]]; then
            echo $(( 2 * i + 3 )) >> "$output_file"
        fi
    done
}

# Worker function: sieve a segment
# Arguments: start end base_primes_file segment_id temp_dir
sieve_segment() {
    local start=$1
    local end=$2
    local base_primes_file=$3
    local segment_id=$4
    local temp_dir=$5
    local output_file="$temp_dir/primes_segment_${segment_id}_$$"
    
    # Initialize segment sieve (track odd numbers relative to start)
    local -a sieve
    local segment_size=$(( (end - start) / 2 + 1 ))
    
    for ((i = 0; i < segment_size; i++)); do
        sieve[$i]=0
    done
    
    # Read base primes and mark composites
    # Skip base_prime=2 since all even numbers are already excluded
    local first=1
    while read -r base_prime; do
        if [[ $first -eq 1 ]]; then
            first=0
            continue  # Skip 2
        fi
        
        # Start from base_prime^2, or first multiple >= start if base_prime^2 < start
        local first_multiple=$(( base_prime * base_prime ))
        
        # Adjust if base_prime^2 is less than start
        if (( first_multiple < start )); then
            first_multiple=$(( (start + base_prime - 1) / base_prime * base_prime ))
        fi
        
        # Adjust to odd number if needed
        if (( first_multiple % 2 == 0 )); then
            ((first_multiple += base_prime))
        fi
        
        # Calculate index in sieve
        local idx=$(( (first_multiple - start) / 2 ))
        
        # Mark all multiples
        for ((j = idx; j < segment_size; j += base_prime)); do
            sieve[$j]=1
        done
    done < "$base_primes_file"
    
    # Write results to output file
    local num=$start
    if (( num % 2 == 0 )); then
        ((num++))
    fi
    
    for ((i = 0; i < segment_size; i++)); do
        if [[ ${sieve[$i]} -eq 0 ]]; then
            echo "$num" >> "$output_file"
        fi
        ((num += 2))
    done
}

# Count logical processors
count_processors() {
    if [[ -f /proc/cpuinfo ]]; then
        grep -c '^processor' /proc/cpuinfo 2>/dev/null || echo 2
    elif [[ "$(uname)" == "Darwin" ]]; then
        sysctl -n hw.ncpu 2>/dev/null || echo 2
    else
        echo 2  # Default fallback
    fi
}

# Parallel sieve main function
primes_parallel() {
    local limit=$1
    local output_format="${2:-list}"
    local use_parallel=true
    local num_workers=0
    
    # Parse optional flags: --parallel [workers], --no-parallel, --workers N
    shift 2 2>/dev/null || true
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --parallel)
                use_parallel=true
                if [[ "$2" =~ ^[0-9]+$ ]]; then
                    num_workers=$2
                    shift
                fi
                shift
                ;;
            --no-parallel)
                use_parallel=false
                shift
                ;;
            --workers)
                use_parallel=true
                shift
                if [[ "$1" =~ ^[0-9]+$ ]]; then
                    num_workers=$1
                    shift
                fi
                ;;
            *)
                # Positional args done
                break
                ;;
        esac
    done
    
    # Validate input
    if [[ ! "$limit" =~ ^[0-9]+$ ]]; then
        echo "Error: limit must be a non-negative integer" >&2
        return 1
    fi
    
    if [[ "$limit" -lt 2 ]]; then
        echo "Error: limit must be >= 2" >&2
        return 1
    fi
    
    # Auto-detect workers if not specified
    if [[ $num_workers -le 0 ]]; then
        num_workers=$(count_processors)
        ((num_workers > 4)) && num_workers=4  # Cap at 4 to avoid overhead
    fi
    
    # For small limits, don't use parallel (overhead not worth it)
    if [[ "$limit" -lt 10000 ]]; then
        use_parallel=false
    fi
    
    if [[ "$use_parallel" != "true" ]]; then
        # Fallback to single-threaded version
        source "${BASH_SOURCE[0]%/*}/primes.sh" 2>/dev/null || true
        if declare -f primes_up_to >/dev/null 2>&1; then
            primes_up_to "$limit" "$output_format"
        else
            echo "Error: single-threaded version not available" >&2
            return 1
        fi
        return 0
    fi
    
    # Clean up temp files on exit
    local temp_dir="/tmp/primes_parallel_$$"
    mkdir -p "$temp_dir"
    trap "rm -rf '$temp_dir'" EXIT
    
    local base_primes_file="$temp_dir/base_primes.txt"
    local sqrt_limit
    sqrt_limit=$(echo "sqrt($limit)" | bc)
    
    # Generate base primes
    generate_base_primes "$sqrt_limit" "$base_primes_file"
    
    # Segment size based on limit and workers - each worker gets roughly equal share
    local segment_size=$(( (limit / num_workers) / 2 ))
    ((segment_size < 5000)) && segment_size=5000
    ((segment_size > 100000)) && segment_size=100000
    
    # Create segment files
    local segment_id=0
    local start=3
    
    while [[ $start -le $limit ]]; do
        local end=$((start + segment_size - 1))
        ((end > limit)) && end=$limit
        
        # Start worker in background
        sieve_segment "$start" "$end" "$base_primes_file" "$segment_id" "$temp_dir" &
        ((segment_id++))
        
        # Limit concurrent workers
        if (( segment_id >= num_workers )); then
            wait
            segment_id=0
        fi
        
        ((start += segment_size))
    done
    
    # Wait for remaining workers
    wait
    
    # Collect all results - segment files are named primes_segment_N_PID
    local all_primes_file="$temp_dir/all_primes.txt"
    for f in "$temp_dir"/primes_segment_*; do
        [[ -f "$f" ]] && cat "$f"
    done | sort -n > "$all_primes_file"
    
    # Output results
    # Add 2 as the first prime (segments start from 3)
    if [[ "$output_format" == "count" ]]; then
        local prime_count=$(wc -l < "$all_primes_file" | tr -d ' ')
        echo $((prime_count + 1))
    else
        echo "2"
        cat "$all_primes_file"
    fi
}

# Main entry point
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    if [[ "$1" == "--help" || "$1" == "-h" ]]; then
        cat <<EOF
Calculate primes up to a given limit using parallel Sieve of Eratosthenes

Usage: $0 <limit> [format] [--parallel [workers]]
  format: 'list' (default) or 'count'
  workers: number of parallel workers (default: auto-detect)

Examples:
  $0 100 list      # list all primes up to 100
  $0 100 count     # count primes up to 100
  $0 1000000       # list primes up to 1M (auto-parallel)
  $0 1000000 --no-parallel  # single-threaded
  $0 1000000 --parallel 4   # use 4 workers

Note: Uses segmented sieve with background workers for parallelism
EOF
        exit 0
    fi
    
    if [[ $# -lt 1 ]]; then
        echo "Usage: $0 <limit> [format] [--parallel [workers]]" >&2
        exit 1
    fi
    
    # Parse arguments
    limit="${1//_/}"
    shift
    
    if [[ ! "$limit" =~ ^[0-9]+$ ]]; then
        echo "Error: limit must be a non-negative integer" >&2
        exit 1
    fi
    
    if [[ "$limit" -lt 2 ]]; then
        echo "Error: limit must be >= 2" >&2
        exit 1
    fi
    
    # Get format (if provided) - can be positional or --format flag
    # After shifting once, $1 is the format if it's not a flag
    if [[ $# -ge 1 && "$1" != --* ]]; then
        format="$1"
        shift
    else
        format="list"
    fi
    
    # Use single-threaded for small limits (overhead not worth it)
    if [[ "$limit" -lt 10000 ]]; then
        source "${BASH_SOURCE[0]%/*}/primes.sh" 2>/dev/null || true
        if declare -f primes_up_to >/dev/null 2>&1; then
            primes_up_to "$limit" "$format"
        else
            echo "Error: single-threaded version not available" >&2
            exit 1
        fi
        exit 0
    fi
    
    primes_parallel "$limit" "$format" "$@"
fi
