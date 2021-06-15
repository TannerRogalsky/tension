#![allow(unused)]

use solstice_2d::solstice::{self, Context};

pub struct Resources {
    pub sans_font_data: Vec<u8>,
}

pub struct LoadedResources {
    pub sans_font: solstice_2d::FontId,
}

impl Resources {
    pub fn try_into_loaded(
        self,
        ctx: &mut Context,
        gfx: &mut solstice_2d::Graphics,
    ) -> eyre::Result<LoadedResources> {
        use std::convert::TryInto;

        Ok(LoadedResources {
            sans_font: gfx.add_font(self.sans_font_data.try_into()?),
        })
    }
}

pub enum ImageDataRepr {
    Bytes(Vec<u8>),
    #[cfg(target_arch = "wasm32")]
    ImageElement(web_sys::HtmlImageElement),
}

pub struct ImageData {
    pub data: ImageDataRepr,
    pub width: u32,
    pub height: u32,
    pub format: solstice::PixelFormat,
}

impl ImageData {
    fn try_into_image(
        self,
        ctx: &mut Context,
        nearest: bool,
    ) -> eyre::Result<solstice::image::Image> {
        use solstice::{
            image::{Image, Settings},
            texture::TextureType,
        };
        let ImageData {
            data,
            width,
            height,
            format,
        } = self;
        let settings = Settings {
            mipmaps: false,
            filter: if nearest {
                solstice::texture::FilterMode::Nearest.into()
            } else {
                solstice::texture::FilterMode::Linear.into()
            },
            wrap: solstice::texture::WrapMode::Repeat.into(),
            ..Default::default()
        };
        let img = match data {
            ImageDataRepr::Bytes(data) => Image::with_data(
                ctx,
                TextureType::Tex2D,
                format,
                width,
                height,
                &data,
                settings,
            )?,
            #[cfg(target_arch = "wasm32")]
            ImageDataRepr::ImageElement(data) => Image::with_html_image(
                ctx,
                TextureType::Tex2D,
                format,
                width,
                height,
                &data,
                settings,
            )?,
        };
        Ok(img)
    }
}
