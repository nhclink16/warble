//! A fixed-length delay line.

/// `read` gives the sample written `len` writes ago. Call `read` then `write` once per sample.
pub struct Delay {
    buf: Vec<f32>,
    pos: usize,
}

impl Delay {
    pub fn new(len: usize) -> Delay {
        Delay {
            buf: vec![0.0; len.max(1)],
            pos: 0,
        }
    }

    pub fn seconds(sample_rate: f32, seconds: f32) -> Delay {
        Delay::new((sample_rate * seconds).round() as usize)
    }

    #[inline]
    pub fn read(&self) -> f32 {
        self.buf[self.pos]
    }

    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}
