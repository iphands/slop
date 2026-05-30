# local-wikipedia-mcp — Execution Plan (Verbose)

## 1. Vision

Build a fully offline, local-first MCP server that allows AI agents to query Wikipedia knowledge without internet access.

The system:
- Ingests official Wikimedia dumps
- Processes and cleans articles
- Generates embeddings for semantic retrieval
- Stores in Qdrant (hybrid search)
- Exposes retrieval tools via MCP
- Runs fully in Docker, reproducible and testable

---

## 2. Core Principles

- **Rust-only implementation**
- **Small incremental commits (task-level)**
- **Always test + lint before commit**
- **Everything must be modular and reusable**
- **No monolithic crates**
- **Every pipeline stage must be independently runnable**
- **Offline-first by design**
- **Rebuildable from raw dumps deterministically**

---

## 3. Repository Layout

```
./crates/
  lib_common/        # shared types, error handling, utils
  lib_wiki_parse/    # XML parsing + markup cleanup
  lib_chunking/      # article chunking logic
  lib_embeddings/    # embedding generation interface
  lib_vector/        # Qdrant client wrapper + hybrid search
  lib_mcp/           # MCP protocol server logic
  bin_ingest/        # ingestion CLI
  bin_server/        # MCP server binary

./context/
  plans/            # numbered execution plans
  pitfalls.md       # failures + lessons
  current_state.md  # restart instructions / system state

./scratch/wikimcp   # generated artifacts (NOT committed)

./ai/data/wikipedia # read-only dump input

./Dockerfile
./bin/build_container
./bin/run_container
./justfile
```

---

## 4. High-Level Architecture

```
Wikipedia Dump
      ↓
XML Parser (lib_wiki_parse)
      ↓
Clean Articles
      ↓
Chunker (lib_chunking)
      ↓
Embedding Pipeline (lib_embeddings)
      ↓
Vector Store (Qdrant + metadata)
      ↓
Hybrid Search Layer
      ↓
MCP Server (tools API)
      ↓
AI Agent
```

---

## 5. MCP Tool Surface

Expose the following tools:

### Required Tools

- `wiki_search(query)`
  - lexical + metadata search

- `wiki_semantic_search(query)`
  - embedding-based retrieval via Qdrant

- `wiki_read(title)`
  - exact article lookup

- `wiki_related(title)`
  - graph-based + embedding similarity expansion

---

## 6. Data Flow Design

### 6.1 Ingestion Pipeline

Steps:

1. Load Wikimedia dump (`/ai/data/wikipedia/*`)
2. Parse XML dump stream
3. Extract:
   - title
   - text
   - redirect info
4. Clean MediaWiki markup
5. Resolve redirects
6. Normalize article structure

---

### 6.2 Chunking Strategy

- Chunk by section headings first
- Fallback to token-based chunking
- Target chunk size: 300–800 tokens
- Overlap: ~10–15%

Each chunk:
- article_id
- chunk_id
- section_path
- text
- metadata

---

### 6.3 Embedding Pipeline

- Generate embeddings per chunk
- Store:
  - vector
  - metadata (title, section, offsets)
- Batch processing supported
- Re-embeddings must be idempotent

---

### 6.4 Vector Store (Qdrant)

- Hybrid search:
  - dense vectors
  - optional sparse keyword index
- Metadata filters:
  - article title
  - namespace
  - section path

---

## 7. MCP Server Design

### Responsibilities

- Accept MCP requests
- Route to retrieval layer
- Format results for agents
- Enforce limits (token caps, result caps)

### Transport

- Start with **stdio (local dev)**
- Add optional HTTP/SSE server mode

Port:
- `8008`

---

## 8. Containerization

### Docker Requirements

- Full reproducible build
- Mount:
  ```
  /scratch:/scratch:ro
  ```

### Scripts

- `./bin/build_container`
- `./bin/run_container`

---

## 9. Build System

### Justfile targets

- `just build`
- `just test`
- `just lint`
- `just run`
- `just docs`

### Docs

- Rustdoc required for all crates
- No undocumented public APIs allowed

---

## 10. Testing Strategy

- Unit tests for:
  - parser correctness
  - chunking boundaries
  - embedding pipeline consistency
  - vector store queries

- Integration tests:
  - ingestion → Qdrant → MCP retrieval

- Deterministic test fixtures from small wiki dumps

---

## 11. Development Rules

### REQUIRED

- Many small commits (task-level granularity)
- Each task must be independently testable
- Lint + test before commit
- Prefer composition over monoliths
- Avoid premature optimization but design for scale

---

## 12. Context Management System

### /context/plans/

- One file per plan iteration:
  ```
  NN_<title>.md
  ```

- Must be continuously updated

### /context/pitfalls.md

- Record:
  - parsing issues
  - embedding failures
  - Qdrant quirks
  - performance bottlenecks

### /context/current_state.md

YOU ARE COMPLETING THIS PROJECT IN A RAPLH LOOP
If you restart you will restart without context and need to pick up and drive to completion

In this current_state file you **must include**:
  - What you were doing
  - What you think is left
  - What is the most high prio concern
  
As you restart first look at dirty state in the repo and try to unserstand it, commit, and then move forward!

---

## 13. Execution Phases

### Phase 1 — Foundation
- Rust workspace setup
- basic crates
- CLI skeleton
- Docker scaffold

### Phase 2 — Wikipedia Parsing
- XML ingestion
- markup cleaning
- redirect resolution

### Phase 3 — Chunking + Embeddings
- chunk pipeline
- embedding generation
- storage format

### Phase 4 — Qdrant Integration
- schema design
- indexing
- hybrid search

### Phase 5 — MCP Server
- tool API
- stdio transport
- HTTP optional mode

### Phase 6 — Hardening
- tests
- benchmarks
- docs
- reproducibility

---

## 14. Future Work

- Multi-language Wikipedia support
- Wiktionary integration
- Wikidata graph expansion
- Cross-document knowledge graph
- Citation-aware retrieval

---

## 15. Key Success Criteria

- Fully offline operation
- <200ms semantic search latency (local)
- Deterministic rebuild from dump
- Works with multiple MCP-compatible agents
- Simple `docker run` experience
