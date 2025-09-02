use std::{
    f64::consts::PI,
    ops::{Add, Sub},
};

use geo::{GeoFloat, Intersects, Line, LineString, Point, Polygon, Rect};

use crate::{
    l,
    math::{pt_rect_postion, PtRectPostion},
    p,
};

// FIX: Using the slower gradient descent algorithms until we fully explore a fast and accurate optimization
use super::gradient_descent::meridian_to_meridian;

/// Internal struct for ensuring Lng wrapping math works correctly.
#[derive(Debug, Clone, Copy)]
struct Lng<T: GeoFloat>(T);

impl<T: GeoFloat> From<T> for Lng<T> {
    fn from(n: T) -> Self {
        let pi = T::from(PI).unwrap();
        // Must be in radians in the domain [-Pi, Pi]
        let n = (n + pi) % (T::from(2).unwrap() * pi);

        Lng(n - (n.signum() * pi))
    }
}

impl<T: GeoFloat> From<Lng<T>> for f64 {
    fn from(n: Lng<T>) -> Self {
        n.0.to_f64().unwrap()
    }
}

impl<T: GeoFloat> PartialEq for Lng<T> {
    // Equal if the underlying f64 are equal, or if they are on PI/-PI
    fn eq(&self, other: &Self) -> bool {
        let pi = T::from(PI).unwrap();
        self.0 == other.0 || self.0.abs() == pi && other.0.abs() == pi
    }
}

impl<T: GeoFloat> Add for Lng<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Lng::from(self.0 + rhs.0)
    }
}

impl<T: GeoFloat> Sub for Lng<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Lng::from(self.0 - rhs.0)
    }
}

/// Calculate the great circle distance between two [`Point`]'s using the Haversine formula.
///
/// Inputs and outputs are in radians. Convert radians to a linear distance by multiplying by the sphere's radius.
pub fn haversine<T: GeoFloat>(p1: &Point<T>, p2: &Point<T>) -> T {
    let two = T::one() + T::one();
    let dlat = p2.y() - p1.y();
    let dlon = p2.x() - p1.x();

    let a = (dlat / two).sin().powi(2) + p1.y().cos() * p2.y().cos() * (dlon / two).sin().powi(2);
    two * a.sqrt().asin()
}

/// Calculate the great circle distance between a [`Point`] and a [`Line`] using the Haversine formula.
///
/// Inputs and outputs are in radians. Convert radians to a linear distance by multiplying by the sphere's radius.
///
/// Adapted from [TurfJS](https://github.com/Turfjs/turf/blob/master/packages/turf-point-to-line-distance/index.ts).
pub fn haversine_pt_line<T: GeoFloat>(pt: &Point<T>, line: &Line<T>) -> T {
    // Projection logic is identical to the euclidean case,
    // but distance calc is different
    let (x, y) = pt.x_y();
    let (x1, y1) = line.start_point().x_y();

    let (x2, y2) = line.end_point().x_y();

    let (a, b, c, d) = (x - x1, y - y1, x2 - x1, y2 - y1);

    let dot = a * c + b * d;
    let len_sq = c * c + d * d;

    // Wrap in an `if` to account for a zero line length
    // Just has to be <0 to work so we pick distance to p1
    let param = if len_sq == T::zero() {
        -T::one()
    } else {
        dot / len_sq
    };

    if param < T::zero() {
        // Closest to start point, so reduces to pt-pt
        haversine(pt, &line.start_point())
    } else if param > T::one() {
        // Closest to end point, so pt-pt again
        haversine(pt, &line.end_point())
    } else {
        // Here we project onto the segment
        let projected = p!(x1 + param * c, y1 + param * d);
        haversine(pt, &projected)
    }
}

/// Calculate the great circle distance between a [`Point`] and a [`LineString`] using the Hsversine formula.
///
/// Iterates through each segment in the [`LineString`] to find the closest.
///
/// Inputs and outputs are in radians. Convert radians to a linear distance by multiplying by the sphere's radius.
pub fn haversine_pt_linestring<T: GeoFloat>(pt: &Point<T>, linestring: &LineString<T>) -> T {
    let mut min_dist = T::infinity();

    for segment in linestring.lines() {
        let d = haversine_pt_line(pt, &segment);
        if d < min_dist {
            min_dist = d;
        }
    }

    min_dist
}

/// Calculate the great circle distance between a [`Point`] and a [`Rect`] using the Haversine formula.
///
/// Inputs and outputs are in radians. Convert radians to a linear distance by multiplying by the sphere's radius.
pub fn haversine_pt_rect<T: GeoFloat>(pt: &Point<T>, rect: &Rect<T>) -> T {
    // The action depends on which of the eight surrounding locations the comparison point falls within
    match pt_rect_postion(rect, pt) {
        PtRectPostion::Contained => T::zero(),
        // Straight latitude differences the closest approach will be a longitude meridian
        PtRectPostion::North => pt.y() - rect.max().y,
        PtRectPostion::South => rect.min().y - pt.y(),
        // WARN: This method changed to East-West version using point-to-line based on fixing underlying logic
        // This should be reassessed before finalizing, including implementing some prop tests
        // East use the rect's right edge
        PtRectPostion::East | PtRectPostion::NorthEast | PtRectPostion::SouthEast => {
            haversine_pt_line(
                pt,
                &l!(rect.max().x, rect.min().y, rect.max().x, rect.min().y),
            )
        }
        // West use the rect's left edge
        PtRectPostion::West | PtRectPostion::NorthWest | PtRectPostion::SouthWest => {
            haversine_pt_line(
                pt,
                &l!(rect.min().x, rect.min().y, rect.min().x, rect.max().y),
            )
        }
    }
}

/// Calculate the great circle distance between two [`Rect`]'s using the
/// Haversine formula.
///
/// Inputs and outputs are in radians. Convert radians to a linear distance by
/// multiplying by the sphere's radius.
///
/// Note that the antimeridian is a problem that is not easy to solve, see:
/// https://macwright.com/2016/09/26/the-180th-meridian.html. All calcs
/// in this module assume that no shape can cross 180 deg lng. Everything
/// must be a separate shape in a composite.
pub fn haversine_rect_rect<T: GeoFloat>(r1: &Rect<T>, r2: &Rect<T>) -> T {
    // Overlap logic works the same as Euclidean
    let overlap_x = r1.max().x >= r2.min().x && r2.max().x >= r1.min().x;
    let overlap_y = r1.max().y >= r2.min().y && r2.max().y >= r1.min().y;

    match (overlap_x, overlap_y) {
        // If any overlap, then 0
        (true, true) => T::zero(),
        // If x (lng) overlaps, then find the closest pair of lats and
        // return the difference - no need to run through haversine
        // as latitude math maps directly to radians
        (true, false) => {
            let d1 = (r1.min().y - r2.max().y).abs();
            let d2 = (r2.min().y - r1.max().y).abs();

            if d1 < d2 {
                d1
            } else {
                d2
            }
        }
        // If y (lat) overlaps, then find the point of overlap with the
        // maximum abs value of lat (closest to the poles) and calc
        // distance for this lat and the respective lngs
        // TODO: When optimizing it may turn out that more aggressive approximations are fine, like using the old
        // version of taking the common point with the highest abs lat
        (false, _) => {
            // Easiest way to adjust for lng wrapping is to take the pair
            // with the min lng delta, because Lng::sub deals with wrapping
            let delta_xa = f64::from(Lng::from(r1.max().x) - Lng::from(r2.min().x)).abs();
            let delta_xi = f64::from(Lng::from(r1.min().x) - Lng::from(r2.max().x)).abs();

            let (l1, l2) = if delta_xa < delta_xi {
                (
                    l!(r1.max().x, r1.min().y, r1.max().x, r1.max().y),
                    l!(r2.min().x, r2.min().y, r2.min().x, r2.max().y),
                )
            } else {
                (
                    l!(r1.min().x, r1.min().y, r1.min().x, r1.max().y),
                    l!(r2.max().x, r2.min().y, r2.max().x, r2.max().y),
                )
            };

            meridian_to_meridian(&l1, &l2).0
        } // When neither overlaps, take the distance from the closest
          // corners, accounting for wrapping lngs
          // FIX: This branch eliminated due to the observation that there will be cases where nearest endpoints are not
          // necessarily the closest
          /*(false, false) => {
              // Easiest way to adjust for lng wrapping is to take the pair
              // with the min lng delta, because Lng::sub deals with wrapping
              let delta_xa = f64::from(Lng::from(r1.max().x) - Lng::from(r2.min().x)).abs();
              let delta_xi = f64::from(Lng::from(r1.min().x) - Lng::from(r2.max().x)).abs();

              let (x1, x2) = if delta_xa < delta_xi {
                  (r1.max().x, r2.min().x)
              } else {
                  (r1.min().x, r2.max().x)
              };
              let (y1, y2) = if r1.max().y < r2.min().y {
                  (r1.max().y, r2.min().y)
              } else {
                  (r1.min().y, r2.max().y)
              };

              haversine(&p!(x1, y1), &p!(x2, y2))
          }*/
    }
}

/// Calculate the great circle distance between a [`Point`] and an arbitrary [`Polygon`] using the
/// Haversine formula.
///
/// Inputs and outputs are in radians. Convert radians to a linear distance by
/// multiplying by the sphere's radius.
pub fn haversine_pt_poly<T: GeoFloat>(pt: &Point<T>, poly: &Polygon<T>) -> T {
    // Distance is 0 if it intersects anywhere in the polygon
    // Otherwise find the ring with the smallest distance, inside or out
    if poly.intersects(pt) {
        T::zero()
    } else {
        let mut dist = haversine_pt_linestring(pt, poly.exterior());
        for ring in poly.interiors() {
            dist = dist.min(haversine_pt_linestring(pt, ring));
        }

        dist
    }
}

#[cfg(test)]
mod test {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8};

    use approx::assert_abs_diff_eq;

    use crate::{
        harness::{read_cities, read_city_pairs},
        EARTH_RADIUS_METERS,
    };

    use super::{super::gradient_descent, *};

    #[test]
    fn haversine_is_correct() {
        let cities: Vec<_> = read_cities().collect();
        let test_results = read_city_pairs();

        for (a, pa) in &cities {
            for (b, pb) in &cities {
                // TODO: This clone should not be necessary, but seems difficult to avoid
                // Best solution seems to be to replace with hashbrown, but probably not useful for testing
                let target = test_results.get(&(a.clone(), b.clone())).unwrap();
                let result = EARTH_RADIUS_METERS * haversine(pa, pb);

                approx::assert_abs_diff_eq!(result, target, epsilon = 1e-2);
            }
        }
    }

    #[test]
    fn create_eq_add_subtract_lngs() {
        // Into works for f64, from works for Lng
        let l1 = Lng::from(FRAC_PI_2);
        let l2 = Lng::from(-FRAC_PI_2);

        // Basic equals works
        assert_eq!(FRAC_PI_2, f64::from(l1));
        assert_eq!(-FRAC_PI_2, f64::from(l2));

        // Wrap into +-PI
        let l3 = Lng::from(3.0 * PI / 2.0);
        assert_eq!(-FRAC_PI_2, f64::from(l3));

        // -PI == PI, and implements PartialEq
        let l4 = Lng::from(PI);

        let l5 = Lng::from(-PI);
        assert_eq!(l4, l5);

        // Can add and subtract in a wrapping manner
        let l7 = Lng::from(FRAC_PI_4);
        let l8 = Lng::from(FRAC_PI_4 * 3.0);
        assert_eq!(l1 + l7, l8);
        assert_eq!(l4 + l1, l2);
        assert_eq!(l1 - l7, l7);
        assert_eq!(l7 - l1, Lng::from(-FRAC_PI_4));

        // Check that wrapping subtraction works for large negatives
        // Using an approximate match to avoid floating point issues
        let res: f64 =
            (Lng::from(-7.0 * FRAC_PI_8) - Lng::from(7.0 * FRAC_PI_8) - Lng::from(FRAC_PI_4))
                .into();
        assert!(res < 1e-6);
    }

    #[test]
    fn rect_dist_works_for_simple_rects() {
        // 0.4 is approx pi/8
        let b1 = Rect::new(p!(0.1, -0.1), p!(0.5, 0.4));

        // Test an overlap
        let b2 = Rect::new(p!(0.2, 0.0), p!(1.0, 0.8));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), 0.0, epsilon = 1e-6);

        // Test touching
        let b2 = Rect::new(p!(0.5, 0.0), p!(0.6, 0.2));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), 0.0, epsilon = 1e-6);

        // Test lat above - simple as the distance should just be the delta in radians
        let b2 = Rect::new(p!(0.1, 0.6), p!(0.3, 0.8));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), 0.2, epsilon = 1e-6);

        // Test lat below
        let b2 = Rect::new(p!(0.1, -0.4), p!(0.3, -0.5));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), 0.3, epsilon = 1e-6);

        // Test lng greater than, min dist @ 0.4, b2 ends higher
        let b2 = Rect::new(p!(0.6, 0.2), p!(0.8, 0.6));
        let (d, _, _) = gradient_descent::meridian_to_meridian(
            &l!(0.5, -0.1, 0.5, 0.4),
            &l!(0.6, 0.2, 0.6, 0.6),
        );
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);

        // Test lng greater than, min dist @ -0.1, b2 starts lower
        let b2 = Rect::new(p!(0.7, -0.2), p!(0.8, -0.1));
        let (d, _, _) = gradient_descent::meridian_to_meridian(
            &l!(0.5, -0.1, 0.5, 0.4),
            &l!(0.7, -0.2, 0.7, -0.1),
        );
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);

        // Test lng less than - min dist @ 0.3, b1 ends higher
        let b2 = Rect::new(p!(-0.2, 0.0), p!(-0.1, 0.3));
        let (d, _, _) = gradient_descent::meridian_to_meridian(
            &l!(0.1, -0.1, 0.1, 0.4),
            &l!(-0.1, 0.0, -0.1, 0.3),
        );
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);

        // Test corner - top left
        let b2 = Rect::new(p!(-0.2, -0.3), p!(-0.1, -0.2));
        let d = haversine(&p!(0.1, -0.1), &p!(-0.1, -0.2));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);

        // Test corner - top right
        let b2 = Rect::new(p!(0.8, -0.4), p!(0.9, -0.3));
        let d = haversine(&p!(0.5, -0.1), &p!(0.8, -0.3));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);

        // Test corner - bottom right
        let b2 = Rect::new(p!(0.9, 0.6), p!(1.0, 0.7));
        let d = haversine(&p!(0.5, 0.4), &p!(0.9, 0.6));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);

        // Test corner - bottom left
        let b2 = Rect::new(p!(-0.8, 0.7), p!(-0.6, 0.8));
        let d = haversine(&p!(0.1, 0.4), &p!(-0.6, 0.7));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);
    }

    #[test]
    fn rect_dist_works_when_on_other_side_of_antimeridian() {
        let b1 = Rect::new(p!(2.9, 0.0), p!(3.0, 0.4));

        // Test overlapping latitude
        let b2 = Rect::new(p!(-3.0, 0.1), p!(-2.9, 0.3));
        let (d, _, _) = gradient_descent::meridian_to_meridian(
            &l!(3.0, 0.0, 3.0, 0.4),
            &l!(-3.0, 0.1, -3.0, 0.3),
        );
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);

        // Sanity check on overall distance - much less than a circle
        assert!(d < PI / 8.0);

        // Test corner
        let b2 = Rect::new(p!(-2.8, -0.4), p!(-2.7, -0.2));
        let d = haversine(&p!(3.0, 0.0), &p!(-2.8, 0.2));
        assert_abs_diff_eq!(haversine_rect_rect(&b1, &b2), d, epsilon = 1e-6);
    }

    fn test_poly() -> Polygon {
        Polygon::new(
            LineString::from(vec![(0.0, 0.0), (0.0, 0.6), (0.6, 0.6), (0.6, 0.0)]),
            vec![LineString::from(vec![
                (0.2, 0.2),
                (0.2, 0.4),
                (0.4, 0.4),
                (0.4, 0.2),
            ])],
        )
    }

    #[test]
    fn dist_pt_poly_is_zero_when_inside_and_on_boundaries() {
        let p1 = Point::new(0.1, 0.1);
        let p2 = Point::new(0.1, 0.0);

        let poly = test_poly();

        assert_eq!(0.0, haversine_pt_poly(&p1, &poly));
        assert_eq!(0.0, haversine_pt_poly(&p2, &poly));
    }

    #[test]
    fn dist_pt_poly_is_exterior_distance_when_outside() {
        let pt = Point::new(0.7, 0.1);
        let poly = test_poly();

        // The line to test

        let line = Line::from([(0.6, 0.6), (0.6, 0.0)]);

        let dist = haversine_pt_poly(&pt, &poly);
        let test = haversine_pt_line(&pt, &line);

        assert_abs_diff_eq!(dist, test, epsilon = 1e-6);
    }

    #[test]
    fn dist_pt_poly_is_interior_when_inside_inner_ring() {
        let pt = Point::new(0.25, 0.3);
        let poly = test_poly();

        // The line to test
        let line = Line::from([(0.2, 0.2), (0.2, 0.4)]);

        let dist = haversine_pt_poly(&pt, &poly);
        let test = haversine_pt_line(&pt, &line);
        assert_abs_diff_eq!(dist, test, epsilon = 1e-6);
    }
}
