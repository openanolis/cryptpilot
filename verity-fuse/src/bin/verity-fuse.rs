use anyhow::{bail, Context, Result};
use cap_std::fs::Dir;
use clap::Parser;
use fuser::MountOption;
use relative_path::{RelativePath, RelativePathBuf};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;
use verity_fuse::file_verifier::{FileVerifier, VerifiableFile};
use verity_fuse::{cli::Cli, filesystem::VerityFS};

const ROOT_INO: u64 = 1;
const ROOT_PATH: &str = "";

/// Passthrough file entry - no verification
#[derive(Debug)]
struct PassthroughFile {
    ino: u64,
    path: RelativePathBuf,
}

impl VerifiableFile for PassthroughFile {
    fn ino(&self) -> u64 {
        self.ino
    }

    fn path(&self) -> &RelativePath {
        &self.path
    }

    fn expected_size(&self) -> Option<u64> {
        // Passthrough mode - return None for directories, Some(0) for files
        // This is not accurate but enough for passthrough testing
        Some(0)
    }

    fn block_size(&self) -> Option<u32> {
        Some(4096)
    }

    fn verify_data_block(&self, _block_index: usize, _data: &[u8]) -> Result<()> {
        // Passthrough mode - always succeeds (unsafe!)
        Ok(())
    }
}

/// Passthrough verifier for testing - loads file tree from filesystem
/// Does NOT perform actual verification (always returns Ok)
#[derive(Debug)]
struct PassthroughVerifier {
    path_to_ino: HashMap<RelativePathBuf, u64>,
    ino_to_file: HashMap<u64, PassthroughFile>,
}

impl PassthroughVerifier {
    fn new(source: &Path) -> Result<Self> {
        let dir = Dir::open_ambient_dir(source, cap_std::ambient_authority())?;

        let mut path_to_ino = HashMap::new();
        let mut ino_to_file = HashMap::new();
        let mut next_ino = ROOT_INO;

        // Insert root
        let root_path = RelativePathBuf::from(ROOT_PATH);
        path_to_ino.insert(root_path.clone(), ROOT_INO);
        // Root is a directory, not a file - don't add to ino_to_file
        next_ino += 1;

        // Preload entire tree
        Self::preload_recursive(
            &dir,
            &mut path_to_ino,
            &mut ino_to_file,
            &mut next_ino,
            &root_path,
        )?;

        Ok(Self {
            path_to_ino,
            ino_to_file,
        })
    }

    fn preload_recursive(
        dir: &Dir,
        path_to_ino: &mut HashMap<RelativePathBuf, u64>,
        ino_to_file: &mut HashMap<u64, PassthroughFile>,
        next_ino: &mut u64,
        dir_path: &RelativePath,
    ) -> Result<()> {
        let mut stack = vec![dir_path.to_owned()];

        while let Some(current_path) = stack.pop() {
            // Read directory entries
            let entries = dir
                .read_dir(relative_path_to_path(&current_path))
                .with_context(|| format!("failed to read dir: {current_path:?}"))?;

            for entry in entries.flatten() {
                let name = entry.file_name();
                let child_path = current_path.join(
                    RelativePath::from_path(Path::new(&name))
                        .with_context(|| format!("invalid dir entry name: {name:?}"))?,
                );

                // Allocate inode for this entry if not exists
                if !path_to_ino.contains_key(&child_path) {
                    let ino = *next_ino;
                    *next_ino += 1;
                    path_to_ino.insert(child_path.clone(), ino);

                    // Only add regular files to ino_to_file
                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        ino_to_file.insert(
                            ino,
                            PassthroughFile {
                                ino,
                                path: child_path.clone(),
                            },
                        );
                    }

                    info!(ino, ?child_path, "allocated inode");
                }

                // Recurse into directories
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    stack.push(child_path);
                }
            }
        }

        Ok(())
    }
}

impl FileVerifier for PassthroughVerifier {
    type File = PassthroughFile;

    fn lookup_by_ino(&self, ino: u64) -> Option<&PassthroughFile> {
        self.ino_to_file.get(&ino)
    }

    fn lookup_by_path(&self, path: &RelativePath) -> Option<&PassthroughFile> {
        let ino = self.path_to_ino.get(path)?;
        self.ino_to_file.get(ino)
    }
}

fn relative_path_to_path(relative_path: &RelativePath) -> std::path::PathBuf {
    relative_path.to_logical_path(".")
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    if !cli.source.exists() {
        bail!("Source path does not exist: {:?}", cli.source);
    }
    if !cli.source.is_dir() {
        bail!("Source path is not a directory: {:?}", cli.source);
    }

    if !cli.mount_point.exists() {
        bail!("Mount point does not exist: {:?}", cli.mount_point)
    }

    // Create passthrough verifier (loads file tree, no verification)
    info!("Loading file tree from {:?}", cli.source);
    let verifier =
        PassthroughVerifier::new(&cli.source).context("Failed to create passthrough verifier")?;

    let fs = VerityFS::new(&cli.source, verifier).context("Failed to create verity-fuse fs")?;

    info!(
        source = ?cli.source,
        mount_point = ?cli.mount_point,
        "Starting verity-fuse with passthrough verifier (NO VERIFICATION - for testing only)"
    );

    fuser::mount2(
        fs,
        &cli.mount_point,
        &[
            MountOption::RO,
            MountOption::FSName("verity-fuse".into()),
            MountOption::AllowOther,
            MountOption::NoAtime, // Reduce noise
        ],
    )?;

    info!("Exited successfully.");

    Ok(())
}
