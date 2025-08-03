//! Newton's method approximation functions for numerical solutions to distance problems.
//!
//! Numerical approximation is required when there is no analytical solution to nearest points. This module contains
//! implementations of Newton's method for simple approximation methodologies. These generally converge fast, but can be
//! temperamental and are therefore tested against the slower but more robust gradient descent functions.

use geo::{GeoFloat, Line, Point};

use crate::p;

use super::{haversine, MAX_ITERATIONS_MSG, VALID_GF};

const DEBUG_DISPLAY: bool = false;
// TODO: Consider switching to scale aware tolerance and delta, and also accounting for precision in geofloats
const TOLERANCE: f64 = 1e-6;
const DELTA: f64 = 1e-7;

/// Find the minimum distance between a meridian segments and a point using Newton's method optimization.
///
/// # Arguments
/// * `lat_a` - Minimum latitude of the line segment (in radians)
/// * `lat_b` - Maximum latitude of the line segment (in radians)  
/// * `lon` - Constant longitude of the line segment (in radians)
/// * `point` - Target point to find distance to
///
/// # Returns
/// Minimum distance in radians and the final matched point
///
/// # Assumptions
/// - point is inside the latitude bounds of the line represented by `lat_a` and `lat_b`
/// - point is not on the line segment
///
/// This method heavily relies on the nature of the specific meridian-to-point problem, requiring that the problem has:
/// - A single minimum point within the domain, and no other critical points
/// - If the minimum is inside the domain, then the function is also convex
/// - If the minimum is outside the domain the curve will be monotonic
///
/// This allows the initial shortcutting based on monotonicity detection and aggressive stepping as there will never be
/// any local minima.
pub(super) fn meridian_to_point<T: GeoFloat>(
    lat_a: T,
    lat_b: T,
    lon: T,
    point: &Point<T>,
) -> (T, Point<T>) {
    if DEBUG_DISPLAY {
        eprintln!(
            "Inputs: lat_a = {:.6} lat_b = {:.6} lon = {:.6} | pt = {:.6}, {:.6}",
            lat_a.to_f64().unwrap(),
            lat_b.to_f64().unwrap(),
            lon.to_f64().unwrap(),
            point.x().to_f64().unwrap(),
            point.y().to_f64().unwrap()
        );
    }

    // Setup, including ensuring correct ordering of the input lat range
    let tolerance = T::from(TOLERANCE).expect(VALID_GF);
    let delta = T::from(DELTA).expect(VALID_GF);
    let half = T::from(0.5).expect(VALID_GF);
    let two = T::one() + T::one();

    let lat_min = lat_a.min(lat_b);
    let lat_max = lat_a.max(lat_b);

    // This function should only be used when the point is inside the lat bounds of the line
    debug_assert!(point.y() <= lat_max && point.y() >= lat_min);

    // Constrain the latitude domain for calculation - this works as the function has a well defined relationship where
    // the great circle distance will be closest within a reasonable range of the point
    // TODO: Confirm that this works, potentially using prop testing, want it to be as tight as possible
    let lon_range = T::from(3.0).expect(VALID_GF) * (point.x() - lon).abs();
    let lat_min = lat_min.max(point.y() - lon_range);
    let lat_max = lat_max.min(point.y() + lon_range);

    // For ease of computation, we consider optimizing d = f(t) where t [0, 1] rather than reproducing the latitudes
    // everywhere, also providing a closure that easily converts to a latitude where required
    let mut t: T;

    // The closure lets us easily calculate d = f(t) in a single step
    let dist_ft = |t: T| haversine(&p!(lon, lat_min + t * (lat_max - lat_min)), point);

    // Test whether the minimum distance lies at either of the endpoints
    let dist_at_min = dist_ft(T::zero());
    let dist_at_min_fwd = dist_ft(delta);
    let grad_at_min = (dist_at_min_fwd - dist_at_min) / delta;

    // If the gradient at the minimum latitude is > 0 this point must be the minimum as it is impossible for there to be
    // an internal minimum or the other end being a minimum while all the known properties hold
    if grad_at_min > T::zero() {
        if DEBUG_DISPLAY {
            eprintln!(
                "Gradient at min lat = {:+.6e}, min dist = {:.6}",
                grad_at_min.to_f64().unwrap(),
                dist_at_min.to_f64().unwrap(),
            );
        }

        // In debug mode, determine whether or not we are constraining the range - will be used to ensure that we are no
        // both constraining the range, then falsely claiming that the minimum distance is at the end of the range
        debug_assert!(lat_min <= lat_a.min(lat_b));
        return (dist_at_min, p!(lon, lat_min));
    }

    let dist_at_max = dist_ft(T::one());
    let dist_at_max_back = dist_ft(T::one() - delta);
    let grad_at_max = (dist_at_max - dist_at_max_back) / delta;

    // Similarly, if the gradient at the max latitude is < 0 this point must be the minimum
    if grad_at_max < T::zero() {
        if DEBUG_DISPLAY {
            eprintln!(
                "Gradient at max lat = {:+.6e}, min dist = {:.6}",
                grad_at_max.to_f64().unwrap(),
                dist_at_max.to_f64().unwrap(),
            );
        }

        // See notes above
        debug_assert!(lat_max >= lat_a.max(lat_b));
        return (dist_at_max, p!(lon, lat_max));
    }

    if DEBUG_DISPLAY {
        eprintln!("  i     dist       lat        t_new | bound conv  |        dt         ddt");
    }

    // Now seed the initial t guess at the halfway point
    t = half;

    // Now when we get to iteration we know that we have an internal minimum point
    // TODO: Confirm the best number of iterations to use here based on prop testing
    for i in 0..20 {
        // Current guess of point on line
        let distance = dist_ft(t);

        // Numerical derivatives using central difference
        // Central difference is more accurate but slightly more calculations than one-sided stepping
        let t_plus = (t + delta).min(T::one());
        let t_minus = (t - delta).max(T::zero());

        let dist_plus = dist_ft(t_plus);
        let dist_minus = dist_ft(t_minus);

        let first_derivative = (dist_plus - dist_minus) / (two * delta);
        let second_derivative = (dist_plus - two * distance + dist_minus) / (delta * delta);

        // Newton's method update: t_new = t - f'(t) / f''(t)
        // TODO: Confirm that the use of gradient descent here works, maybe with prop testing
        let t_new = if second_derivative.abs() < tolerance {
            // Use gradient descent if second derivative is near zero
            t - T::from(0.01).expect(VALID_GF) * first_derivative
        } else {
            // TODO: Fix if keeping step check
            t - first_derivative / second_derivative
        };

        // Clamp to [0, 1] bounds then test convergence
        // FIX: We should know that we are not converged at the boundary by definition
        let t_clamped = t_new.clamp(T::zero(), T::one());
        let hit_boundary = (t_clamped - t_new).abs() > T::epsilon();
        let converged = if hit_boundary {
            let at_lower = t_clamped <= T::epsilon();
            let boundary_grad = if at_lower { grad_at_min } else { grad_at_max };

            (at_lower && boundary_grad > T::zero())
                || (!at_lower && boundary_grad < T::zero())
                || boundary_grad.abs() < tolerance
        } else {
            (t_clamped - t).abs() < tolerance
        };

        if DEBUG_DISPLAY {
            eprintln!(
                "{:3} {:.6} {:+.6} {:.10} | {:5} {:5} | {:+.6e} {:+.6e}",
                i,
                distance.to_f64().unwrap(),
                (lat_min + t * (lat_max - lat_min)).to_f64().unwrap(),
                t_new.to_f64().unwrap(),
                hit_boundary,
                converged,
                first_derivative.to_f64().unwrap(),
                second_derivative.to_f64().unwrap(),
            );
        }

        // Check convergence then do a final distance calculation
        if converged {
            let final_lat = lat_min + t_clamped * (lat_max - lat_min);
            return (dist_ft(t_clamped), p!(lon, final_lat));
        }

        // When not converged and also at a boundary, then we bisect to restart, otherwise take the newton rec
        t = if hit_boundary {
            (t + t_clamped) / two
        } else {
            t_clamped
        };
    }

    // TODO: Consider if we want to failover gracefully in production
    panic!("{}", MAX_ITERATIONS_MSG);
}

/// Find the minimum distance between two meridian segments using Newton's method optimization.
///
/// # Arguments
/// * `l1` - The first segment, must have constant x value (in radians)
/// * `l2` - The second segment, must have constant x value (in radians)  
///
/// # Returns
/// Minimum distance in radians
///
/// # Assumptions
/// - both of the provided segments are meridians (i.e., same x-value at start and end
/// - the segments overlap
pub(super) fn meridian_to_meridian<T: GeoFloat>(l1: &Line<T>, l2: &Line<T>) -> T {
    debug_assert_eq!(l1.dx(), T::zero());
    debug_assert_eq!(l2.dx(), T::zero());

    // Sort endpoints by latitude
    let l1_min = l1.start.y.min(l1.end.y);
    let l1_max = l1.start.y.max(l1.end.y);
    let l2_min = l2.start.y.min(l2.end.y);
    let l2_max = l2.start.y.max(l2.end.y);

    // Check overlap
    let overlap_min = l1_min.max(l2_min);
    let overlap_max = l1_max.min(l2_max);

    debug_assert!(
        overlap_min <= overlap_max,
        "provided line segments do not overlap"
    );

    // Pick the one with larger absolute value (closer to a pole)
    let (chosen_pt, other_line) = if overlap_min.abs() >= overlap_max.abs() {
        if l1_min > l2_min {
            (p!(l1.start.x, l1_min), l2)
        } else {
            (p!(l2.start.x, l2_min), l1)
        }
    } else {
        if l1_max < l2_max {
            (p!(l1.start.x, l1_max), l2)
        } else {
            (p!(l2.start.x, l2_max), l1)
        }
    };

    if DEBUG_DISPLAY {
        eprintln!("point: {:?} line: {:?}", chosen_pt, other_line);
    }

    // Then the problem reduces to a line_to_point optimization
    let (d, _) = meridian_to_point(
        other_line.start.y,
        other_line.end.y,
        other_line.start.x,
        &chosen_pt,
    );

    d
}

#[cfg(test)]
mod tests {
    use super::super::gradient_descent;
    use super::*;
    use approx::assert_abs_diff_eq;
    use std::f64::consts::PI;

    /// Constant for a single degree.
    const DEGREE: f64 = 0.01745;

    mod point_vertical_line {
        use super::*;

        #[test]
        fn point_directly_west() {
            // Line: longitude 0°, latitude 0° to 30°
            // Point: longitude -45°, latitude 15°
            let lat_min = 0.0;
            let lat_max = PI / 6.0; // 30°
            let lon = 0.0; // 0°
            let point = Point::new(-PI / 12.0, PI / 12.0); // -45°, 15°
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point from numerical solution
            let expected_point = Point::new(0.0, 0.2706);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn point_directly_east() {
            // Line: longitude 90°, latitude -20° to 20°
            // Point: longitude 120°, latitude 0°
            let lat_min = -PI / 9.0; // -20°
            let lat_max = PI / 9.0; // 20°
            let lon = PI / 2.0; // 90°
            let point = Point::new(2.0 * PI / 3.0, 0.0); // 120°, 0°
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (90°, 0°)
            let expected_point = Point::new(PI / 2.0, 0.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn closest_point_at_line_midpoint() {
            // Line: longitude 0°, latitude -10° to 10°
            // Point: longitude 45°, latitude 0° (equidistant from endpoints)
            let lat_min = -PI / 18.0; // -10°
            let lat_max = PI / 18.0; // 10°
            let lon = 0.0; // 0°
            let point = Point::new(PI / 4.0, 0.0); // 45°, 0°
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to midpoint (0°, 0°)
            let expected_point = Point::new(0.0, 0.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn antimeridian_crossing() {
            // Line: longitude 179°, latitude -30° to 30°
            // Point: longitude -170° (10° west of antimeridian), latitude 0°
            let lat_min = -PI / 6.0; // -30°
            let lat_max = PI / 6.0; // 30°
            let lon = 179.0 * PI / 180.0; // 179°
            let point = Point::new(-170.0 * PI / 180.0, 0.0); // -170°, 0°
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (179°, 0°)
            let expected_point = Point::new(179.0 * PI / 180.0, 0.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn high_latitude_line() {
            // Line: longitude 0°, latitude 60° to 80°
            // Point: longitude 90°, latitude 70°
            let lat_min = PI / 3.0; // 60°
            let lat_max = 4.0 * PI / 9.0; // 80°
            let lon = 0.0; // 0°
            let point = Point::new(PI / 2.0, 7.0 * PI / 18.0); // 90°, 70°
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (0°, 80°)
            let expected_point = Point::new(0.0, 4.0 * PI / 9.0);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn southern_hemisphere() {
            // Line: longitude -90°, latitude -60° to -30°
            // Point: longitude -120°, latitude -45°
            let lat_min = -PI / 3.0; // -60°
            let lat_max = -PI / 6.0; // -30°
            let lon = -PI / 2.0; // -90°
            let point = Point::new(-2.0 * PI / 3.0, -PI / 4.0); // -120°, -45°
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            // Expected: distance to point (-90°, -45°)
            let expected_point = Point::new(-PI / 2.0, -0.8571);
            let expected = haversine(&point, &expected_point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        // This condition gives us the case where the distance is far enough that the closest point is the south pole
        // FIX: Think though this, it may solve the issue for the test in the newton module, but the newton case for the
        // same thing seems to be working, so not sure what the deal is with the failure on the convergence in knn
        #[test]
        fn southern_hemisphere_sydney() {
            let lat_min = -PI / 2.0;
            let lat_max = 0.0;
            let lon = 0.0;
            let point = p!(2.639100, -0.591122); // approx sydney
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            let (expected, ep) = gradient_descent::merdian_to_point(lat_min, lat_max, lon, &point);
            eprintln!("Expected: {} {:?}", expected, ep);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }

        #[test]
        fn close_longitude_long_segment() {
            let lat_min = 0.0;
            let lat_max = 1.0;
            let lon = 0.1;
            let point = Point::new(0.11, 0.2);
            let (d, _) = meridian_to_point(lat_min, lat_max, lon, &point);

            let (expected, _) = gradient_descent::merdian_to_point(lat_min, lat_max, lon, &point);

            assert_abs_diff_eq!(d, expected, epsilon = 1e-6);
        }
    }

    mod vertical_line_line {
        use super::*;

        #[test]
        fn parallel_vertical_lines_same_longitude() {
            // Two vertical lines at the same longitude should have zero distance
            let l1 = Line::new((0.0, -0.3), (0.0, 0.3));
            let l2 = Line::new((0.0, -0.5), (0.0, 0.5));

            let d = meridian_to_meridian(&l1, &l2);
            assert_abs_diff_eq!(d, 0.0, epsilon = 1e-6);
        }

        #[test]
        fn parallel_vertical_lines_different_longitude() {
            // Two vertical lines at different longitudes (radians)
            let l1 = Line::new((0.0, -1.0), (0.0, 1.0));
            let l2 = Line::new((DEGREE, -1.0), (DEGREE, 1.0));

            let distance = meridian_to_meridian(&l1, &l2);

            let (expected, _, _) = gradient_descent::meridian_to_meridian(&l1, &l2);

            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
        }

        #[test]
        fn line_line_antimeridian_crossing() {
            let half = DEGREE / 2.0;
            // One line near π, another near -π (crossing antimeridian)
            let l1 = Line::new((PI - half, -0.1), (PI - half, 0.1)); // ~179.5° in radians
            let l2 = Line::new((-PI + half, -0.1), (-PI + half, 0.1)); // ~-179.5° in radians

            let distance = meridian_to_meridian(&l1, &l2);

            // The shortest distance should be across the antimeridian (~1°)
            // not the long way around (~359°)
            let (expected, _, _) = gradient_descent::meridian_to_meridian(&l1, &l2);
            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);

            // Sanity check: should be much less than halfway around the world
            assert!(distance < PI / 8.0);
        }

        // Test offset lines with a small lon separation but a larger lat range.
        #[test]
        fn offset_vertical_lines_1() {
            // Lines that are offset in both longitude and latitude (radians)
            let l1 = Line::new((0.0, 0.0), (0.0, 0.05));
            let l2 = Line::new((DEGREE, -0.02), (DEGREE, 0.1));

            let distance = meridian_to_meridian(&l1, &l2);

            // The closest points should be somewhere in the overlapping latitude range
            // Which we add as an assert for a sanity check, but use grad desc as primary solution
            let (expected, p1, p2) = gradient_descent::meridian_to_meridian(&l1, &l2);

            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
            assert!(p1.y() >= 0.0 && p1.y() <= 0.05);
            assert!(p2.y() >= 0.02 && p2.y() <= 0.1);
        }

        // Test more even lon and lat spread.
        #[test]
        fn offset_vertical_lines_2() {
            // Lines that are offset in both longitude and latitude (radians)
            let l1 = Line::new((0.0, 0.0), (0.0, 0.55));
            let l2 = Line::new((20.0 * DEGREE, -0.02), (20.0 * DEGREE, 0.2));

            let distance = meridian_to_meridian(&l1, &l2);

            let (expected, _, _) = gradient_descent::meridian_to_meridian(&l1, &l2);

            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
        }

        // Test small separation in a corner.
        // TODO: Consider moving this test to point-to-line as it fits better there
        #[test]
        fn offset_vertical_lines_3() {
            // Lines that are offset in both longitude and latitude (radians)
            let l1 = Line::new((0.0, 0.0), (0.0, 0.55));
            let l2 = Line::new((20.0 * DEGREE, 0.0), (20.0 * DEGREE, 0.6));

            let distance = meridian_to_meridian(&l1, &l2);
            // The closest points should be somewhere in the overlapping latitude range
            let (expected, _, _) = gradient_descent::meridian_to_meridian(&l1, &l2);

            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
        }

        #[test]
        fn offset_vertical_lines_northern_hemisphere() {
            // Lines offset in both longitude and latitude, away from equator and further apart
            let l1 = Line::new((0.0, 35.0 * DEGREE), (0.0, 70.0 * DEGREE)); // 0° lon, 35° to 70° lat
            let l2 = Line::new((5.0 * DEGREE, 50.0 * DEGREE), (5.0 * DEGREE, 80.0 * DEGREE)); // ~5° lon, 50° to 80° lat

            let distance = meridian_to_meridian(&l1, &l2);
            let (expected, _, _) = gradient_descent::meridian_to_meridian(&l1, &l2);

            // The closest points should be in the overlapping latitude range (50° to 70°)
            assert_abs_diff_eq!(distance, expected, epsilon = 1e-6);
        }
    }
}
