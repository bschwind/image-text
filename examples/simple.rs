use image::{ColorType, DynamicImage, Pixel, Rgba};
use image_text::{AxisAlign, TextBlock, TextBlockPosition};

pub fn main() {
    // Create a gradient image to draw text onto.
    let mut image = DynamicImage::new(512, 512, ColorType::Rgba8);
    let start = Rgba::from_slice(&[0, 0, 255, 255]);
    let end = Rgba::from_slice(&[255, 0, 128, 255]);
    image::imageops::vertical_gradient(&mut image, start, end);

    // Draw the text!
    image_text::draw_text(
        &mut image,
        TextBlock::string("hello world 🌎\nhere is a new line\n日本語 मनीष منش").with_alignment(
            TextBlockPosition {
                x: AxisAlign::CenterAtCanvasCenter,
                y: AxisAlign::CenterAtCanvasCenter,
            },
        ),
    );

    image.save("output.png").unwrap();
}
