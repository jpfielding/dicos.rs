/// Row-major grayscale image buffer.
///
/// Stores pixel data in row-major order (left-to-right, top-to-bottom).
/// Native format is `u16` little-endian for DICOS 16-bit images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrayImage<T: Copy> {
    width: u32,
    height: u32,
    data: Vec<T>,
}

impl<T: Copy> GrayImage<T> {
    /// Creates a new image with the given dimensions, filled with the specified value.
    pub fn new(width: u32, height: u32, fill: T) -> Self {
        let len = (width as usize) * (height as usize);
        Self {
            width,
            height,
            data: vec![fill; len],
        }
    }

    /// Creates a new image from existing pixel data.
    ///
    /// Returns `None` if `data.len() != width * height`.
    pub fn from_data(width: u32, height: u32, data: Vec<T>) -> Option<Self> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            data,
        })
    }

    /// Returns the image width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the image height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns a shared slice over the row-major pixel data.
    #[inline]
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Returns a mutable slice over the row-major pixel data.
    #[inline]
    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// Consumes the image and returns the owned pixel buffer.
    #[inline]
    pub fn into_data(self) -> Vec<T> {
        self.data
    }

    /// Returns the number of pixels in the image.
    #[inline]
    pub fn num_pixels(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// Returns the pixel at the given (x, y) coordinate.
    ///
    /// # Panics
    /// Panics if `x >= width` or `y >= height`.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> T {
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Sets the pixel at the given (x, y) coordinate.
    ///
    /// # Panics
    /// Panics if `x >= width` or `y >= height`.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, value: T) {
        self.data[(y as usize) * (self.width as usize) + (x as usize)] = value;
    }

    /// Returns a reference to the row at the given y coordinate.
    ///
    /// # Panics
    /// Panics if `y >= height`.
    #[inline]
    pub fn row(&self, y: u32) -> &[T] {
        let start = (y as usize) * (self.width as usize);
        &self.data[start..start + self.width as usize]
    }

    /// Returns a mutable reference to the row at the given y coordinate.
    ///
    /// # Panics
    /// Panics if `y >= height`.
    #[inline]
    pub fn row_mut(&mut self, y: u32) -> &mut [T] {
        let start = (y as usize) * (self.width as usize);
        let w = self.width as usize;
        &mut self.data[start..start + w]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fills_with_value() {
        let img = GrayImage::<u16>::new(4, 3, 42);
        assert_eq!(img.width(), 4);
        assert_eq!(img.height(), 3);
        assert_eq!(img.data().len(), 12);
        assert!(img.data().iter().all(|&v| v == 42));
    }

    #[test]
    fn from_data_valid() {
        let data = vec![1u16, 2, 3, 4, 5, 6];
        let img = GrayImage::from_data(3, 2, data.clone()).unwrap();
        assert_eq!(img.width(), 3);
        assert_eq!(img.height(), 2);
        assert_eq!(img.data(), data.as_slice());
    }

    #[test]
    fn from_data_wrong_size() {
        let data = vec![1u16, 2, 3];
        assert!(GrayImage::from_data(2, 2, data).is_none());
    }

    #[test]
    fn pixel_access() {
        let data = vec![10u16, 20, 30, 40, 50, 60];
        let mut img = GrayImage::from_data(3, 2, data).unwrap();
        assert_eq!(img.pixel(0, 0), 10);
        assert_eq!(img.pixel(2, 1), 60);
        img.set_pixel(1, 0, 99);
        assert_eq!(img.pixel(1, 0), 99);
    }

    #[test]
    fn row_access() {
        let data = vec![1u16, 2, 3, 4, 5, 6];
        let img = GrayImage::from_data(3, 2, data).unwrap();
        assert_eq!(img.row(0), &[1, 2, 3]);
        assert_eq!(img.row(1), &[4, 5, 6]);
    }

    #[test]
    fn num_pixels() {
        let img = GrayImage::<u8>::new(10, 20, 0);
        assert_eq!(img.num_pixels(), 200);
    }

    #[test]
    #[should_panic]
    fn pixel_out_of_bounds() {
        let img = GrayImage::<u16>::new(2, 2, 0);
        // Access beyond the last pixel (index 4, vec has 4 elements)
        img.pixel(0, 2);
    }

    #[test]
    fn zero_dimension_image() {
        let img = GrayImage::<u16>::new(0, 5, 0);
        assert_eq!(img.num_pixels(), 0);
        assert!(img.data().is_empty());
    }
}
