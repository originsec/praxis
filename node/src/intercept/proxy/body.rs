use flate2::read::{DeflateDecoder, GzDecoder};
use std::io::{Cursor, Read};

pub use common::MAX_INTERCEPT_CAPTURE_BODY_SIZE as MAX_CAPTURE_BODY_SIZE;

fn bounded_copy(body: &[u8]) -> Vec<u8> {
    if body.len() > MAX_CAPTURE_BODY_SIZE {
        common::log_warn!(
            "Truncating captured body from {} to {} bytes",
            body.len(),
            MAX_CAPTURE_BODY_SIZE
        );
    }
    body[..body.len().min(MAX_CAPTURE_BODY_SIZE)].to_vec()
}

fn read_decompressed<R: Read>(reader: R, encoding: &str, original: &[u8]) -> Vec<u8> {
    let mut limited = reader.take((MAX_CAPTURE_BODY_SIZE + 1) as u64);
    let mut decompressed = Vec::new();
    match limited.read_to_end(&mut decompressed) {
        Ok(_) if decompressed.len() <= MAX_CAPTURE_BODY_SIZE => decompressed,
        Ok(_) => {
            common::log_warn!(
                "Skipping {} decompression because the result exceeded {} bytes",
                encoding,
                MAX_CAPTURE_BODY_SIZE
            );
            bounded_copy(original)
        }
        Err(error) => {
            common::log_debug!("Failed to decompress {} body: {}", encoding, error);
            bounded_copy(original)
        }
    }
}

pub fn decompress_grpc_payload(payload: &[u8]) -> Vec<u8> {
    if payload.len() < 5 {
        return bounded_copy(payload);
    }

    let compressed = payload[0] != 0;
    let message_len = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]) as usize;

    let Some(message_end) = 5usize.checked_add(message_len) else {
        return bounded_copy(payload);
    };
    if payload.len() < message_end {
        return bounded_copy(payload);
    }

    let message_data = &payload[5..message_end];

    if compressed {
        read_decompressed(
            GzDecoder::new(Cursor::new(message_data)),
            "gRPC gzip",
            message_data,
        )
    } else {
        bounded_copy(message_data)
    }
}

pub fn decompress_body(body: &[u8], content_encoding: Option<&str>) -> Vec<u8> {
    let Some(encoding) = content_encoding else {
        return bounded_copy(body);
    };

    let encoding = encoding.to_lowercase();
    if encoding.contains("gzip") {
        read_decompressed(GzDecoder::new(body), "gzip", body)
    } else if encoding.contains("deflate") {
        read_decompressed(DeflateDecoder::new(body), "deflate", body)
    } else if encoding.contains("br") {
        read_decompressed(
            brotli::Decompressor::new(Cursor::new(body), 4096),
            "brotli",
            body,
        )
    } else if encoding.contains("zstd") {
        match zstd::stream::read::Decoder::new(Cursor::new(body)) {
            Ok(decoder) => read_decompressed(decoder, "zstd", body),
            Err(error) => {
                common::log_debug!("Failed to initialize zstd decoder: {}", error);
                bounded_copy(body)
            }
        }
    } else {
        bounded_copy(body)
    }
}
