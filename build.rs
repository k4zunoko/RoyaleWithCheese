use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // OpenCV DLLのソースディレクトリ
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let opencv_bin_dir = Path::new(&manifest_dir)
        .join("third_party")
        .join("opencv")
        .join("build")
        .join("x64")
        .join("vc16")
        .join("bin");

    // OpenCV DLLディレクトリが存在するか確認
    if !opencv_bin_dir.exists() {
        println!(
            "cargo:warning=OpenCV DLL directory not found: {}",
            opencv_bin_dir.display()
        );
        return;
    }

    // ビルドプロファイルに応じた出力ディレクトリを決定
    let out_dir = env::var("OUT_DIR").unwrap();
    let target_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3) // OUT_DIR is target/<profile>/build/<pkg>/out, so go up 3 levels to target/<profile>
        .unwrap();

    // OpenCV DLLファイルをコピー
    copy_opencv_dlls(&opencv_bin_dir, target_dir);

    // Spout DLLをコピー
    copy_spout_dlls(&manifest_dir, target_dir);

    // Spoutリンカー設定
    let spout_lib_dir = Path::new(&manifest_dir)
        .join("third_party")
        .join("spoutdx-ffi")
        .join("lib");
    println!("cargo:rustc-link-search=native={}", spout_lib_dir.display());

    println!("cargo:rerun-if-changed=third_party/opencv/build/x64/vc16/bin");
    println!("cargo:rerun-if-changed=third_party/spoutdx-ffi");
}

fn copy_opencv_dlls(src_dir: &Path, dst_dir: &Path) {
    let entries = match fs::read_dir(src_dir) {
        Ok(entries) => entries,
        Err(e) => {
            println!(
                "cargo:warning=Failed to read OpenCV DLL directory: {}",
                e
            );
            return;
        }
    };

    let mut copied_count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(filename) = path.file_name() {
            let filename_str = filename.to_string_lossy();
            
            // "opencv"で始まるDLLファイルをコピー
            if filename_str.ends_with(".dll") && filename_str.starts_with("opencv") {
                let dst_path = dst_dir.join(filename);
                
                // すでに同じサイズの同名ファイルが存在する場合はスキップ
                if dst_path.exists() {
                    if let (Ok(src_meta), Ok(dst_meta)) = (fs::metadata(&path), fs::metadata(&dst_path)) {
                        if src_meta.len() == dst_meta.len() {
                            continue;
                        }
                    }
                }

                match fs::copy(&path, &dst_path) {
                    Ok(_) => {
                        println!("cargo:warning=Copied: {} -> {}", filename_str, dst_path.display());
                        copied_count += 1;
                    }
                    Err(e) => {
                        println!("cargo:warning=Failed to copy DLL {}: {}", filename_str, e);
                    }
                }
            }
        }
    }

    if copied_count > 0 {
        println!("cargo:warning=Copied {} OpenCV DLLs", copied_count);
    }
}

fn copy_spout_dlls(manifest_dir: &str, target_dir: &Path) {
    let spout_bin_dir = Path::new(manifest_dir)
        .join("third_party")
        .join("spoutdx-ffi")
        .join("bin");
    
    if !spout_bin_dir.exists() {
        println!("cargo:warning=Spout DLL directory not found: {}", spout_bin_dir.display());
        return;
    }
    
    // spoutdx_ffi.dll をコピー
    let dll_path = spout_bin_dir.join("spoutdx_ffi.dll");
    if dll_path.exists() {
        let dst_path = target_dir.join("spoutdx_ffi.dll");
        
        // すでに同じサイズのファイルが存在する場合はスキップ
        if dst_path.exists() {
            if let (Ok(src_meta), Ok(dst_meta)) = (fs::metadata(&dll_path), fs::metadata(&dst_path)) {
                if src_meta.len() == dst_meta.len() {
                    return;
                }
            }
        }
        
        match fs::copy(&dll_path, &dst_path) {
            Ok(_) => {
                println!("cargo:warning=Copied: spoutdx_ffi.dll -> {}", dst_path.display());
            }
            Err(e) => {
                println!("cargo:warning=Failed to copy Spout DLL: {}", e);
            }
        }
    } else {
        println!("cargo:warning=spoutdx_ffi.dll not found in {}", spout_bin_dir.display());
    }
}
