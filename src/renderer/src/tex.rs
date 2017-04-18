//! Texture resource.

use error::Result;
use gfx::texture::Info;
use types::{Factory, RawTexture, RawShaderResourceView};

/// Handle to a GPU texture resource.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Texture {
    texture: RawTexture,
    view: RawShaderResourceView,
}

impl Texture {
    /// Builds a new texture with the given raw texture data.
    pub fn new<'d, D: Into<&'d [&'d [u8]]>>(data: D) -> TextureBuilder<'d> {
        TextureBuilder::new(data)
    }
}

/// Builds new textures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureBuilder<'d> {
    data: &'d [&'d [u8]],
    info: Info,
}

impl<'d> TextureBuilder<'d> {
    /// Creates a new `TextureBuilder` with the given raw texture data.
    pub fn new<D: Into<&'d [&'d [u8]]>>(data: D) -> Self {
        use gfx::Bind;
        use gfx::format::SurfaceType;
        use gfx::memory::Usage;
        use gfx::texture::{AaMode, Kind};

        TextureBuilder {
            data: data.into(),
            info: Info {
                kind: Kind::D2(1, 1, AaMode::Single),
                levels: 1,
                format: SurfaceType::R8_G8_B8_A8,
                bind: Bind::empty(),
                usage: Usage::Dynamic,
            },
        }
    }

    /// Sets the number of mipmap levels to generate.
    pub fn with_mip_levels(mut self, val: u8) -> Self {
        self.info.levels = val;
        self
    }

    // pub fn with_solid_color<C: Into<[f32; 4]>>(&mut self, rgba: C) -> &mut Self {
    //     use gfx::texture::{AaMode, Kind};
    //     let color = rgba.into();
    //     let data: [[u8; 4]; 1] = [[(color[0] * 255.0) as u8,
    //                            (color[1] * 255.0) as u8,
    //                            (color[2] * 255.0) as u8,
    //                            (color[3] * 255.0) as u8]];

    //     self.data = Some(data);
    //     self.info.kind = Kind::D2(1, 1, AaMode::Single);
    //     self
    // }

    /// Sets the texture length and width in pixels.
    pub fn with_size(mut self, l: usize, w: usize) -> Self {
        use gfx::texture::{AaMode, Kind};
        self.info.kind = Kind::D2(l as u16, w as u16, AaMode::Single);
        self
    }

    /// Sets whether the texture is mutable or not.
    pub fn is_mutable(mut self, mutable: bool) -> Self {
        use gfx::memory::Usage;

        if mutable {
            self.info.usage = Usage::Dynamic;
        } else {
            self.info.usage = Usage::Data;
        }

        self
    }

    /// Builds and returns the new texture.
    pub fn build(self, fac: &mut Factory) -> Result<Texture> {
        use gfx::Factory;
        use gfx::format::{ChannelType, Swizzle};
        use gfx::texture::ResourceDesc;

        let chan = ChannelType::Srgb;
        let tex = fac.create_texture_raw(self.info, Some(chan), Some(self.data))?;

        let desc = ResourceDesc {
            channel: ChannelType::Srgb,
            layer: None,
            min: 1,
            max: self.info.levels,
            swizzle: Swizzle::new(),
        };

        let view = fac.view_texture_as_shader_resource_raw(&tex, desc)?;

        Ok(Texture {
            texture: tex,
            view: view,
        })
    }
}
