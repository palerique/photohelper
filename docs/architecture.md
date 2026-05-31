# Photohelper Architecture

Photohelper is an AI-powered, high-performance CLI tool built in Rust for automating the photography post-production pipeline. It handles everything from ingesting RAW files to AI-driven aesthetic culling, semantic deduplication, Lightroom-compatible XMP generation, and watermarked JPEG exports.

## 1. High-Level System Architecture

The system is organized into modular Rust crates, communicating with a central SQLite catalog that acts as the source of truth for the entire pipeline.

```mermaid
graph TD
    subgraph "Core Crates"
        CLI["photohelper-cli<br/>(Entrypoint & Orchestration)"]
        Catalog["photohelper-catalog<br/>(State & DB Management)"]
        Export["photohelper-export<br/>(Image Rendering Engine)"]
        AI["photohelper-ai<br/>(ONNX/Tract Model Inference)"]
    end

    subgraph "Storage & IO"
        DB[("catalog.db<br/>(SQLite Local State)")]
        FS["File System<br/>(RAWs, XMPs, JPEGs)"]
    end

    CLI -->|Routes Commands| Catalog
    CLI -->|Triggers Pipeline| Export
    CLI -->|Invokes Scoring/Clustering| AI

    Catalog <-->|SQL Queries & Locks| DB
    Catalog -->|Extracts EXIF/Paths| FS

    AI -->|Writes Scores & Embeddings| DB
    AI -->|Reads Image Pixels| FS

    Export -->|Queries Top Photos| DB
    Export -->|Reads Source & XMP| FS
    Export -->|Writes Final JPEGs| FS
```

## 2. Core Components

*   **photohelper-cli**: The command-line interface built with `clap`. It orchestrates the pipeline steps (`ingest`, `cull`, `dedup`, `develop`, `export`, `run`).
*   **photohelper-catalog**: Manages the local `.photohelper/catalog.db` SQLite database. It handles database migrations, schema definitions, file-system walking, and EXIF metadata extraction.
*   **photohelper-ai**: Encapsulates the machine learning models.
    *   **NIMA (Neural Image Assessment)**: Used during the `cull` step to assign aesthetic float scores (e.g., `5.87`) to each photo.
    *   **CLIP (ViT-B/32)**: Used during the `dedup` step to generate semantic embeddings and calculate cosine-similarity for grouping similar photos into clusters.
*   **photohelper-export**: The rendering engine. It reads RAW/JPEG files, applies basic development settings, enforces long-edge resizing constraints, overlays watermarks, and writes the final output files based on the smart naming convention.

## 3. Pipeline Sequence Flow

The core power of Photohelper is its automated sequential pipeline. Here is how data flows from a raw folder of images to a curated, exported gallery.

```mermaid
sequenceDiagram
    autonumber
    participant User
    participant CLI as photohelper-cli
    participant DB as catalog.db
    participant FS as File System

    User->>CLI: run photohelper-all.sh

    %% Ingest Phase
    CLI->>FS: Walk target directory
    FS-->>CLI: Return image paths and Exif
    CLI->>DB: Insert files and metadata (status ingested)

    %% Cull Phase (AI)
    CLI->>DB: Fetch unscored images
    CLI->>FS: Load image buffers
    CLI->>CLI: Run NIMA Inference
    CLI->>DB: Update aesthetic_score (float)

    %% Dedup Phase (AI)
    CLI->>DB: Fetch unembedded images
    CLI->>FS: Load image buffers
    CLI->>CLI: Run CLIP Inference
    CLI->>CLI: Calculate Cosine Similarity
    CLI->>DB: Update cluster_id and embeddings

    %% Develop Phase
    CLI->>DB: Fetch all catalogued images
    CLI->>FS: Write Lightroom XMP sidecars

    %% Export Phase
    CLI->>DB: Query images (ordered by score and cluster)
    CLI->>FS: Read image and XMP
    CLI->>CLI: Resize and Watermark
    CLI->>FS: Write exported jpegs

    CLI-->>User: Pipeline Complete
```

## 4. Data Model (SQLite)

The local SQLite catalog (`catalog.db`) ensures the tool can resume if interrupted and avoids re-processing files that haven't changed.

**Table:** `photos`
*   `id` (INTEGER PRIMARY KEY)
*   `path` (TEXT UNIQUE): Relative path to the image.
*   `file_size`, `mtime` (INTEGER): For cache invalidation.
*   `width`, `height` (INTEGER): Extracted dimensions.
*   `aesthetic_score` (REAL): The NIMA AI score (e.g. `5.67`).
*   `cluster_id` (INTEGER): Group ID from the deduplication step.
*   `clip_embedding` (BLOB): The semantic vector array.
*   `status` (TEXT): Enum tracking pipeline progress (`ingested`, `scored`, `embedded`, `errored`).

## 5. File Naming & Grouping Strategy

To facilitate native OS sorting (like macOS Finder), Photohelper exports files using a strict naming taxonomy injected during the `export` phase:

Format: `[cluster-XXX-]cull-[XX.XX]-[raw-filename].jpg`
*   **Cluster**: Similar photos grouped by the Dedup phase get a prefix like `cluster-026-`.
*   **Cull**: Padded float representation (`cull-05.87-`) ensures alphabetical sorting aligns with quality descending.
*   **Raw Name**: The original stem `_MG_9912.jpg` is preserved at the tail for traceability.

When sorted alphabetically in Finder, this ensures clusters are grouped together, and inside the cluster, the absolute best AI-rated photo is positioned at the top.
