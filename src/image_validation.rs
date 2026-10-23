//! A small binding to macOS' maintained decoder, with no temporary image files.
#[cfg(target_os = "macos")]
pub fn validate(bytes: &[u8], max_pixels: usize) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::ffi::c_void;
    type Ref = *const c_void;
    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    struct Size {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
        fn CFRelease(value: Ref);
        fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
        fn CFNumberGetValue(number: Ref, kind: isize, out: *mut i64) -> u8;
    }
    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
        fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
        fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
        fn CGImageSourceGetStatus(source: Ref) -> i32;
        static kCGImagePropertyPixelWidth: Ref;
        static kCGImagePropertyPixelHeight: Ref;
        fn CGImageSourceGetStatusAtIndex(source: Ref, index: usize) -> i32;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGImageGetWidth(image: Ref) -> usize;
        fn CGImageGetHeight(image: Ref) -> usize;
        fn CGImageRelease(image: Ref);
        fn CGColorSpaceCreateDeviceRGB() -> Ref;
        fn CGColorSpaceRelease(space: Ref);
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits: usize,
            row: usize,
            space: Ref,
            info: u32,
        ) -> Ref;
        fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
        fn CGContextFlush(context: Ref);
        fn CGContextRelease(context: Ref);
    }
    struct Owned {
        value: Ref,
        release: unsafe extern "C" fn(Ref),
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { (self.release)(self.value) }
        }
    }
    fn own(value: Ref, release: unsafe extern "C" fn(Ref)) -> anyhow::Result<Owned> {
        ensure!(!value.is_null(), "native image decoder rejected data");
        Ok(Owned { value, release })
    }
    // All framework objects remain alive while the synchronous decoder uses
    // the input and raster buffers. Reverse drop order releases them first.
    unsafe {
        let data = own(
            CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len().try_into()?),
            CFRelease,
        )?;
        let source = own(
            CGImageSourceCreateWithData(data.value, std::ptr::null()),
            CFRelease,
        )?;
        let properties = own(
            CGImageSourceCopyPropertiesAtIndex(source.value, 0, std::ptr::null()),
            CFRelease,
        )?;
        let dimension = |key: Ref| -> anyhow::Result<usize> {
            let value = CFDictionaryGetValue(properties.value, key);
            ensure!(!value.is_null(), "image has no dimensions");
            let mut result = 0i64;
            // kCFNumberSInt64Type = 4, as declared by the platform SDK.
            ensure!(
                CFNumberGetValue(value, 4, &mut result) != 0 && result > 0,
                "invalid image dimensions"
            );
            Ok(usize::try_from(result)?)
        };
        let declared_width = dimension(kCGImagePropertyPixelWidth)?;
        let declared_height = dimension(kCGImagePropertyPixelHeight)?;
        ensure!(
            declared_width
                .checked_mul(declared_height)
                .is_some_and(|n| n <= max_pixels),
            "decoded image exceeds configured pixel budget"
        );
        let image = own(
            CGImageSourceCreateImageAtIndex(source.value, 0, std::ptr::null()),
            CGImageRelease,
        )?;
        let width = CGImageGetWidth(image.value);
        let height = CGImageGetHeight(image.value);
        let pixels = width
            .checked_mul(height)
            .context("image dimensions overflow")?;
        ensure!(
            width > 0 && height > 0 && pixels <= max_pixels,
            "decoded image exceeds configured pixel budget"
        );
        let len = pixels.checked_mul(4).context("raster size overflow")?;
        let space = own(CGColorSpaceCreateDeviceRGB(), CGColorSpaceRelease)?;
        let mut raster = vec![0u8; len];
        let context = own(
            CGBitmapContextCreate(
                raster.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                space.value,
                1,
            ),
            CGContextRelease,
        )?;
        CGContextDrawImage(
            context.value,
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: width as f64,
                    height: height as f64,
                },
            },
            image.value,
        );
        CGContextFlush(context.value);
        ensure!(
            CGImageSourceGetStatusAtIndex(source.value, 0) == 0
                && CGImageSourceGetStatus(source.value) == 0,
            "image data is incomplete or invalid"
        );
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn validate(_: &[u8], _: usize) -> anyhow::Result<()> {
    anyhow::bail!("image validation is currently qualified on macOS only")
}
