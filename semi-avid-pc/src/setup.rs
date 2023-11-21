use ark_ec::pairing::Pairing;
use ark_poly_commit::kzg10::{Powers, UniversalParams, VerifierKey};

/// Specializes the public parameters for a given maximum degree `d` for polynomials
/// `d` should be less that `pp.max_degree()`.
///
/// > see [`ark-poly-commit::kzg10::tests::KZG10`](https://github.com/jdetchart/poly-commit/blob/master/src/kzg10/mod.rs#L509)
pub fn trim<E: Pairing>(
    pp: UniversalParams<E>,
    supported_degree: usize,
) -> Result<(Powers<'static, E>, VerifierKey<E>), ark_poly_commit::Error> {
    let powers_of_g = pp.powers_of_g[..=supported_degree].to_vec();
    let powers_of_gamma_g = (0..=supported_degree)
        .map(|i| pp.powers_of_gamma_g[&i])
        .collect();

    let powers = Powers {
        powers_of_g: ark_std::borrow::Cow::Owned(powers_of_g),
        powers_of_gamma_g: ark_std::borrow::Cow::Owned(powers_of_gamma_g),
    };
    let vk = VerifierKey {
        g: pp.powers_of_g[0],
        gamma_g: pp.powers_of_gamma_g[&0],
        h: pp.h,
        beta_h: pp.beta_h,
        prepared_h: pp.prepared_h.clone(),
        prepared_beta_h: pp.prepared_beta_h.clone(),
    };

    Ok((powers, vk))
}
