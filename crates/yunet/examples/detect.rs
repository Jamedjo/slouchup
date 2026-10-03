//! Usage: detect <model.onnx> <image.png>
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let model = std::fs::read(args.next().expect("model path")).expect("read model");
    let image = image::open(args.next().expect("image path"))
        .expect("open image")
        .to_rgb8();
    let (width, height) = (image.width() as usize, image.height() as usize);
    let detector = yunet::Detector::new(&model, width, height).expect("load model");
    for _ in 0..3 {
        let start = Instant::now();
        let faces = detector
            .detect(image.as_raw(), width, height)
            .expect("detect");
        println!("{:?} {faces:?}", start.elapsed());
    }
}
