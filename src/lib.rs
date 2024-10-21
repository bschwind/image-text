use cosmic_text::{
    Align, Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
};
use fontdb::Family;
use image::{GenericImage, ImageBuffer, Luma, Rgba};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {}

pub fn draw_text<I: GenericImage<Pixel = Rgba<u8>>>(image: &mut I, text_block: TextBlock) {
    let mut text_painter = TextPainter::new();
    text_painter.paint_text_block(image, text_block);
}

pub struct TextPainter {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl Default for TextPainter {
    fn default() -> Self {
        Self::new()
    }
}

impl TextPainter {
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        Self::new_with_font_db(db)
    }

    pub fn new_with_font_db(font_database: fontdb::Database) -> Self {
        let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string());
        let font_system = FontSystem::new_with_locale_and_db(locale, font_database);

        let swash_cache = SwashCache::new();

        Self { font_system, swash_cache }
    }

    pub fn paint_text_block<I: GenericImage<Pixel = Rgba<u8>>>(
        &mut self,
        image: &mut I,
        text_block: TextBlock,
    ) {
        let (surface_width, surface_height) = image.dimensions();

        let buffer = {
            let mut buffer = self.shape_text_block(&text_block);
            let measured_width = self.measure_text_block_width(&buffer);

            self.shape_again_if_needed(
                &mut buffer,
                text_block.text_align,
                Some(measured_width),
                text_block.max_height,
            );

            buffer
        };

        enum TextDirection {
            Horizontal,
            Vertical,
        }

        let axis_position = |align: AxisAlign, text_direction: TextDirection| -> f32 {
            match align {
                AxisAlign::StartAt(value) => value,
                AxisAlign::EndAt(value) => {
                    let measurement = match text_direction {
                        TextDirection::Horizontal => self.measure_text_block_width(&buffer),
                        TextDirection::Vertical => self.measure_text_block_height(&buffer),
                    };

                    value - measurement
                },
                AxisAlign::CenterAt(value) => {
                    let measurement = match text_direction {
                        TextDirection::Horizontal => self.measure_text_block_width(&buffer),
                        TextDirection::Vertical => self.measure_text_block_height(&buffer),
                    };
                    value - (measurement / 2.0)
                },
                AxisAlign::CenterAtCanvasCenter => {
                    let (surface_length, measurement) = match text_direction {
                        TextDirection::Horizontal => {
                            (surface_width, self.measure_text_block_width(&buffer))
                        },
                        TextDirection::Vertical => {
                            (surface_height, self.measure_text_block_height(&buffer))
                        },
                    };
                    (surface_length as f32 / 2.0) - (measurement / 2.0)
                },
            }
        };

        let x = axis_position(text_block.alignment.x, TextDirection::Horizontal);
        let y = axis_position(text_block.alignment.y, TextDirection::Vertical);

        self.add_text(image, x, y, &buffer);
    }

    fn add_text<I: GenericImage<Pixel = Rgba<u8>>>(
        &mut self,
        image: &mut I,
        x: f32,
        y: f32,
        buffer: &Buffer,
    ) {
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let scale = 1.0;
                let physical_glyph = glyph.physical((x, y), scale);

                let Some(glyph_image) =
                    self.swash_cache.get_image(&mut self.font_system, physical_glyph.cache_key)
                else {
                    continue;
                };

                let glyph_x = physical_glyph.x + glyph_image.placement.left;
                let glyph_y =
                    run.line_y.round() as i32 + physical_glyph.y - glyph_image.placement.top;

                let glyph_width = glyph_image.placement.width;
                let glyph_height = glyph_image.placement.height;

                match glyph_image.content {
                    SwashContent::Mask | SwashContent::SubpixelMask => {
                        // Grayscale
                        let glyph_luma_image: ImageBuffer<Luma<u8>, &[u8]> =
                            ImageBuffer::from_raw(glyph_width, glyph_height, &glyph_image.data[..])
                                .unwrap();

                        let (r, g, b, _a) = glyph
                            .color_opt
                            .map(|c| c.as_rgba_tuple())
                            .unwrap_or((255, 255, 255, 255));

                        let glyph_rgba_image: ImageBuffer<Rgba<u8>, Vec<u8>> =
                            ImageBuffer::from_fn(glyph_width, glyph_height, |x, y| {
                                let glyph_alpha = glyph_luma_image.get_pixel(x, y)[0];
                                Rgba([r, g, b, glyph_alpha])
                            });

                        image::imageops::overlay(
                            image,
                            &glyph_rgba_image,
                            glyph_x as i64,
                            glyph_y as i64,
                        );
                    },
                    SwashContent::Color => {
                        // Color
                        let glyph_rgba_image: ImageBuffer<Rgba<u8>, &[u8]> =
                            ImageBuffer::from_raw(glyph_width, glyph_height, &glyph_image.data[..])
                                .unwrap();
                        image::imageops::overlay(
                            image,
                            &glyph_rgba_image,
                            glyph_x as i64,
                            glyph_y as i64,
                        );
                    },
                }
            }
        }
    }

    fn shape_text_block(&mut self, text_block: &TextBlock) -> Buffer {
        const DEFAULT_FONT_SIZE: f32 = 32.0;

        let block_font_size =
            text_block.text_spans.first().map(|s| s.font_size).unwrap_or(DEFAULT_FONT_SIZE);
        let block_line_height =
            text_block.text_spans.first().and_then(|s| s.line_height).unwrap_or(1.0);
        let mut buffer = Buffer::new_empty(Metrics::relative(block_font_size, block_line_height));

        let mut default_attrs = Attrs::new();

        if let Some(font) = text_block.font {
            default_attrs = default_attrs.family(Family::Name(font));
        }

        let spans = text_block.text_spans.iter().map(|span| {
            let text: &str = &span.text;
            let mut metrics = default_attrs
                .metrics(Metrics::relative(span.font_size, span.line_height.unwrap_or(1.0)));

            metrics = metrics.weight(Weight(span.font_weight));

            if let Some(font) = span.font {
                metrics = metrics.family(Family::Name(font));
            }

            let (r, g, b, a) = span.color;
            metrics = metrics.color(cosmic_text::Color::rgba(r, g, b, a));

            (text, metrics)
        });

        buffer.set_rich_text(&mut self.font_system, spans, default_attrs, Shaping::Advanced);

        buffer.set_size(&mut self.font_system, text_block.max_width, text_block.max_height);

        for line in &mut buffer.lines {
            let align = match text_block.text_align {
                TextAlign::Left => Align::Left,
                TextAlign::Right => Align::Right,
                TextAlign::End => Align::End,
                TextAlign::Center => Align::Center,
                TextAlign::Justified => Align::Justified,
            };

            line.set_align(Some(align));
        }

        let prune = true;
        buffer.shape_until_scroll(&mut self.font_system, prune);

        buffer
    }

    pub fn measure(&mut self, text_block: &TextBlock) -> (f32, f32) {
        let mut buffer = self.shape_text_block(text_block);
        let width = self.measure_text_block_width(&buffer);
        let height = self.measure_text_block_height(&buffer);

        self.shape_again_if_needed(&mut buffer, text_block.text_align, Some(width), Some(height));

        (width, height)
    }

    fn measure_text_block_width(&self, buffer: &Buffer) -> f32 {
        let width = buffer.layout_runs().fold(0.0, |width, run| run.line_w.max(width));

        width
    }

    fn measure_text_block_height(&self, buffer: &Buffer) -> f32 {
        let total_lines = buffer.layout_runs().count();

        total_lines as f32 * buffer.metrics().line_height
    }

    fn shape_again_if_needed(
        &mut self,
        buffer: &mut Buffer,
        align: TextAlign,
        width_opt: Option<f32>,
        height_opt: Option<f32>,
    ) {
        // We need to shape once, measure the actual width, and then shape again
        // with that width defined as the max_width. This second shaping is only
        // needed if the alignment is center, right, or justified, because cosmic-text
        // Align doesn't work without a max width defined.
        // See - https://github.com/pop-os/cosmic-text/issues/42#issuecomment-1607731931

        if matches!(
            align,
            TextAlign::Center | TextAlign::Right | TextAlign::End | TextAlign::Justified
        ) {
            buffer.set_size(&mut self.font_system, width_opt, height_opt);
            let prune = true;
            buffer.shape_until_scroll(&mut self.font_system, prune);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub alignment: TextBlockPosition,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub text_align: TextAlign,
    pub text_spans: Vec<Text>,
    /// The default font to use for all text spans.
    /// Can be overrided with `Text.font`.
    pub font: Option<&'static str>,
}

/// Determines positioning for a TextBlock.
#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub struct TextBlockPosition {
    /// Where to align the X value of the text block.
    /// For example:
    ///     x: AxisAlign::EndAt(300.0)
    /// will place the right edge of the text block at x = 300.0
    pub x: AxisAlign,
    /// Where to align the Y value of the text block.
    /// For example:
    ///     y: AxisAlign::StartAt(100.0)
    /// will place the top edge of the text block at y = 100.0
    pub y: AxisAlign,
}

/// Units are in pixels.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum AxisAlign {
    /// For X, align the left edge of the text block to this value.
    /// For Y, align the top edge of the text block to this value.
    StartAt(f32),
    /// For X, align the right edge of the text block to this value.
    /// For Y, align the bottom edge of the text block to this value.
    EndAt(f32),
    /// For X or Y, make the center of the text block align on this value.
    CenterAt(f32),
    /// For X or Y, make the center of the text block align to the center of the canvas.
    CenterAtCanvasCenter,
}

impl Default for AxisAlign {
    fn default() -> Self {
        AxisAlign::StartAt(0.0)
    }
}

/// How text is aligned (or justified) within a TextBlock.
#[allow(unused)]
#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub enum TextAlign {
    /// The text will be left-aligned.
    #[default]
    Left,
    /// The text will be right-aligned.
    Right,
    /// The text will be aligned with the "end" of the axis, which
    /// depends on whether the text is left-to-right, or right-to-left.
    End,
    /// The text will be aligned to the center of the TextBlock.
    Center,
    /// The text will be justified on a per-word basis to keep text aligned
    /// with both the left and right sides of the text block.
    Justified,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub font_size: f32,
    pub font_weight: u16,
    // RGBA tuple, pre-multiplied alpha.
    pub color: (u8, u8, u8, u8),
    /// If present, will override the font, if it exists,
    /// specified on `TextBlock`.
    pub font: Option<&'static str>,
    /// line_height is a relative value. Multiply it by the font-size
    /// to arrive at the absolute line height value.
    pub line_height: Option<f32>,
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 32.0,
            font_weight: 400,
            color: (255, 255, 255, 255),
            font: None,
            line_height: None,
        }
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }
}

impl Default for TextBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBlock {
    pub fn new() -> Self {
        Self {
            alignment: TextBlockPosition::default(),
            max_width: None,
            max_height: None,
            text_align: Default::default(),
            text_spans: vec![],
            font: None,
        }
    }

    pub fn string(text: impl Into<String>) -> Self {
        Self {
            alignment: TextBlockPosition::default(),
            max_width: None,
            max_height: None,
            text_align: Default::default(),
            text_spans: vec![Text::new(text)],
            font: None,
        }
    }

    pub fn with_alignment(mut self, alignment: TextBlockPosition) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn with_max_width(mut self, max_width: f32) -> Self {
        self.max_width = Some(max_width);
        self
    }

    pub fn with_max_height(mut self, max_height: f32) -> Self {
        self.max_height = Some(max_height);
        self
    }

    pub fn with_text_blocks(mut self, text_spans: impl Iterator<Item = Text>) -> Self {
        self.text_spans = text_spans.collect();
        self
    }

    pub fn with_font(mut self, font: &'static str) -> Self {
        self.font = Some(font);
        self
    }
}
