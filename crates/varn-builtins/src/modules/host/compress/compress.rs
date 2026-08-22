use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue, VnArray};

pub struct CompressRuntime;

fn zip_dir_recursive(
    writer: &mut zip::ZipWriter<File>,
    base_dir: &Path,
    current_dir: &Path,
) -> Result<(), String> {
    let entries = fs::read_dir(current_dir).map_err(|e| format!("Zip read dir error: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Zip dir entry error: {e}"))?;
        let path = entry.path();
        let rel_path = path.strip_prefix(base_dir).map_err(|e| format!("Zip strip prefix error: {e}"))?;
        let name = rel_path.to_string_lossy().replace('\\', "/");

        if path.is_dir() {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.add_directory(&name, options).map_err(|e| format!("Zip add dir error: {e}"))?;
            zip_dir_recursive(writer, base_dir, &path)?;
        } else {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(&name, options).map_err(|e| format!("Zip start file error: {e}"))?;
            let mut f = File::open(&path).map_err(|e| format!("Zip open file error: {e}"))?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(|e| format!("Zip read file error: {e}"))?;
            writer.write_all(&buf).map_err(|e| format!("Zip write error: {e}"))?;
        }
    }
    Ok(())
}

varn_contract! {
    module: "runtime:compress",
    contract: "src/modules/host/compress/compress_runtime.vn",
    impl CompressRuntime {
        fn gzip(_ctx: &mut dyn NativeCtx, data: &str) -> Result<Vec<VmValue>, String> {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data.as_bytes()).map_err(|e| format!("gzip compression error: {e}"))?;
            let compressed = encoder.finish().map_err(|e| format!("gzip finish error: {e}"))?;
            Ok(compressed.iter().map(|b| VmValue::from_int(*b as i64)).collect())
        }

        fn gunzip(ctx: &mut dyn NativeCtx, bytes: VnArray) -> Result<String, String> {
            let len = bytes.len(ctx);
            let mut raw_bytes = Vec::with_capacity(len);
            for i in 0..len {
                let v = bytes.get(ctx, i).unwrap_or(VmValue::null());
                raw_bytes.push(v.as_int() as u8);
            }
            let mut decoder = GzDecoder::new(&raw_bytes[..]);
            let mut decompressed = String::new();
            decoder.read_to_string(&mut decompressed).map_err(|e| format!("gunzip decompression error: {e}"))?;
            Ok(decompressed)
        }

        fn deflate(_ctx: &mut dyn NativeCtx, data: &str) -> Result<Vec<VmValue>, String> {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data.as_bytes()).map_err(|e| format!("deflate compression error: {e}"))?;
            let compressed = encoder.finish().map_err(|e| format!("deflate finish error: {e}"))?;
            Ok(compressed.iter().map(|b| VmValue::from_int(*b as i64)).collect())
        }

        fn inflate(ctx: &mut dyn NativeCtx, bytes: VnArray) -> Result<String, String> {
            let len = bytes.len(ctx);
            let mut raw_bytes = Vec::with_capacity(len);
            for i in 0..len {
                let v = bytes.get(ctx, i).unwrap_or(VmValue::null());
                raw_bytes.push(v.as_int() as u8);
            }
            let mut decoder = ZlibDecoder::new(&raw_bytes[..]);
            let mut decompressed = String::new();
            decoder.read_to_string(&mut decompressed).map_err(|e| format!("inflate decompression error: {e}"))?;
            Ok(decompressed)
        }

        fn tarCreate(_ctx: &mut dyn NativeCtx, source_dir: &str, tar_path: &str) -> Result<bool, String> {
            let tar_file = File::create(tar_path).map_err(|e| format!("Tar create error: {e}"))?;
            let mut tar_builder = tar::Builder::new(tar_file);
            tar_builder.append_dir_all(".", source_dir).map_err(|e| format!("Tar append error: {e}"))?;
            tar_builder.finish().map_err(|e| format!("Tar finish error: {e}"))?;
            Ok(true)
        }

        fn tarExtract(_ctx: &mut dyn NativeCtx, tar_path: &str, dest_dir: &str) -> Result<bool, String> {
            let tar_file = File::open(tar_path).map_err(|e| format!("Tar open error: {e}"))?;
            let mut archive = tar::Archive::new(tar_file);
            archive.unpack(dest_dir).map_err(|e| format!("Tar unpack error: {e}"))?;
            Ok(true)
        }

        fn zipCreate(_ctx: &mut dyn NativeCtx, source_dir: &str, zip_path: &str) -> Result<bool, String> {
            let zip_file = File::create(zip_path).map_err(|e| format!("Zip create error: {e}"))?;
            let mut writer = zip::ZipWriter::new(zip_file);
            let base = Path::new(source_dir);
            zip_dir_recursive(&mut writer, base, base)?;
            writer.finish().map_err(|e| format!("Zip finish error: {e}"))?;
            Ok(true)
        }

        fn zipExtract(_ctx: &mut dyn NativeCtx, zip_path: &str, dest_dir: &str) -> Result<bool, String> {
            let zip_file = File::open(zip_path).map_err(|e| format!("Zip open error: {e}"))?;
            let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| format!("Zip archive error: {e}"))?;
            archive.extract(dest_dir).map_err(|e| format!("Zip extract error: {e}"))?;
            Ok(true)
        }
    }
}
