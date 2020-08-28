use color_eyre::Result;
use dssim::{DssimImage, ToRGBAPLU, RGBAPLU};
use imgref::{Img, ImgVec};
use load_image::{Image, ImageData};
use std::path::Path;

pub struct SlateDetector {
    width: usize,
    height: usize,
    slate: DssimImage<f32>,
    similarity_algorithm: dssim::Dssim,
}

impl SlateDetector {
    pub fn new<P: AsRef<Path>>(slate_path: P) -> Result<Self> {
        let similarity_algorithm = dssim::Dssim::new();
        let slate_img = load_path(slate_path)?;
        let slate = similarity_algorithm.create_image(&slate_img).unwrap();

        Ok(Self {
            width: slate_img.width(),
            height: slate_img.height(),
            slate,
            similarity_algorithm,
        })
    }

    pub fn is_match(&self, image_buffer: &[u8]) -> bool {
        let frame_img = load_data(image_buffer).unwrap();
        let frame = self.similarity_algorithm.create_image(&frame_img).unwrap();

        let (res, _) = self.similarity_algorithm.compare(&self.slate, frame);
        let val: f64 = res.into();
        let val = (val * 1000f64) as u32;

        val <= 900u32
    }

    pub fn required_image_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}

fn load_data(data: &[u8]) -> Result<ImgVec<RGBAPLU>> {
    let img = load_image::load_image_data(data, false)?;
    Ok(match_img_bitmap(img))
}

fn load_path<P: AsRef<Path>>(path: P) -> Result<ImgVec<RGBAPLU>> {
    let img = load_image::load_image(path.as_ref(), false)?;
    Ok(match_img_bitmap(img))
}

fn match_img_bitmap(img: Image) -> ImgVec<RGBAPLU> {
    match img.bitmap {
        ImageData::RGB8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::RGB16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::RGBA8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::RGBA16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAY8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAY16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAYA8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAYA16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use dssim::*;
    use imgref::*;
    use load_image::ImageData;
    use std::path::Path;

    #[test]
    fn compare_equal_images() {
        let slate_img = load_path("../slate.jpg").unwrap();

        let algo = dssim::Dssim::new();
        let slate = algo.create_image(&slate_img).unwrap();

        let (res, _) = algo.compare(&slate, slate.clone());
        let val: f64 = res.into();

        assert_eq!((val * 1000f64) as u32, 0u32);
    }

    #[test]
    fn compare_diff_images() {
        let slate_img = load_path("../slate.jpg").unwrap();
        let frame_img = load_path("../non-slate.jpg").unwrap();

        let algo = dssim::Dssim::new();
        let slate = algo.create_image(&slate_img).unwrap();
        let frame = algo.create_image(&frame_img).unwrap();

        let (res, _) = algo.compare(&slate, frame);
        let val: f64 = res.into();

        assert_eq!((val * 1000f64) as u32, 7417u32);
    }
}
