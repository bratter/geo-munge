//! Simple 3-dimensional vector.
//!
//! Specifically used for lng/lat to unit-sphere Cartesian conversions.

use geo::{GeoFloat, Point};

#[derive(Debug, Clone, Copy)]
pub struct Vector<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

impl<T: GeoFloat> Vector<T> {
    /// Create a unit Cartesian [`Vector`] from the [`Point`] on the surface of a sphere.
    pub fn unit_vec_from_point(point: Point<T>) -> Self {
        let lat = point.y();
        let lng = point.x();

        Self {
            x: lat.cos() * lng.cos(),
            y: lat.cos() * lng.sin(),
            z: lat.sin(),
        }
    }

    /// Convert a Cartesian [`Vector`] to a lng/lat [`Point`] on a unit sphere.
    ///
    /// Will first ensure that the vector is normalized to ensure accuracy.
    pub fn unit_vec_into_point(&self) -> Point<T> {
        // TODO: This unitization is safe but takes computation that will sometimes be redundant, should we keep?
        let v = self.normalize();
        let lat = v.z.asin();
        let lng = v.y.atan2(v.x);

        Point::new(lng, lat)
    }

    pub fn dot(&self, other: Vector<T>) -> T {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(&self, other: Vector<T>) -> Vector<T> {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    pub fn magnitude(&self) -> T {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn angle_to(&self, other: Vector<T>) -> T {
        let one = T::one();
        let theta = self.dot(other) / (self.magnitude() * other.magnitude());

        theta.clamp(-one, one).acos()
    }

    pub fn normalize(&self) -> Vector<T> {
        let mag = self.magnitude();
        Vector {
            x: self.x / mag,
            y: self.y / mag,
            z: self.z / mag,
        }
    }

    pub fn scale(self, factor: T) -> Vector<T> {
        Vector {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        }
    }
}
