//! Exact positive buffer density, independent of a compositor protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Density {
    numerator: u32,
    denominator: u32,
}
impl Density {
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, &'static str> {
        if numerator == 0 || denominator == 0 {
            return Err("Density must be a positive ratio");
        }
        let (mut a, mut b) = (numerator, denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Ok(Self {
            numerator: numerator / a,
            denominator: denominator / a,
        })
    }
    pub fn integer(scale: u32) -> Result<Self, &'static str> {
        Self::new(scale, 1)
    }
    pub fn numerator(self) -> u32 {
        self.numerator
    }
    pub fn denominator(self) -> u32 {
        self.denominator
    }
    pub fn factor(self) -> f32 {
        self.numerator as f32 / self.denominator as f32
    }
    /// Layer buffers round outward to cover both logical edges. The renderer
    /// maps the resulting physical extent back to the exact logical viewport.
    pub fn physical_size(self, width: u32, height: u32) -> Result<(u32, u32), &'static str> {
        let edge = |value| {
            if value == 0 {
                return Err("Logical dimensions must be nonzero");
            }
            let product = u64::from(value) * u64::from(self.numerator);
            let physical = product.div_ceil(u64::from(self.denominator));
            u32::try_from(physical).map_err(|_| "Physical dimensions overflow")
        };
        Ok((edge(width)?, edge(height)?))
    }
}
impl Default for Density {
    fn default() -> Self {
        Self::ONE
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ratios_are_validated_and_canonical_and_keep_fractional_edges() {
        assert!(Density::new(0, 120).is_err());
        assert!(Density::new(120, 0).is_err());
        let density = Density::new(150, 120).unwrap();
        assert_eq!(density, Density::new(5, 4).unwrap());
        assert_eq!(density.factor(), 1.25);
        assert_eq!(density.physical_size(65, 3).unwrap(), (82, 4));
        assert_eq!(
            Density::new(180, 120)
                .unwrap()
                .physical_size(100, 50)
                .unwrap(),
            (150, 75)
        );
        assert!(density.physical_size(0, 3).is_err());
        assert!(
            Density::integer(2)
                .unwrap()
                .physical_size(u32::MAX, 1)
                .is_err()
        );
        assert_eq!(
            Density::new(u32::MAX, u32::MAX)
                .unwrap()
                .physical_size(u32::MAX, 1)
                .unwrap(),
            (u32::MAX, 1)
        );
    }
}
