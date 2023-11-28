use ark_ec::pairing::Pairing;
use ark_ff::PrimeField;
use ark_poly::DenseUVPolynomial;
use ark_std::ops::Div;
use ark_std::Zero;

/// split a sequence of raw bytes into valid field elements
///
/// [`split_data_into_field_elements`] supports padding the output vector of
/// elements by giving a number that needs to divide the length of the vector.
pub(crate) fn split_data_into_field_elements<E: Pairing>(
    bytes: &[u8],
    modulus: usize,
) -> Vec<E::ScalarField> {
    let mut elements = Vec::new();

    for chunk in bytes.chunks((E::ScalarField::MODULUS_BIT_SIZE as usize) / 8) {
        elements.push(E::ScalarField::from_le_bytes_mod_order(chunk));
    }

    if elements.len() % modulus != 0 {
        elements.resize(
            (elements.len() / modulus + 1) * modulus,
            E::ScalarField::zero(),
        );
    }

    elements
}

// create a set of polynomials containing k coefficients (#polynomials = |elements| / k)
//
// # Implementation
// as Dragoon uses FEC encoding to share data over a network of peers, we have
// some contraints on the way we compute the polynomials from the data.
//
// with a *(k, n)* code, the output of the encoding mixes all the coefficients
// of the original data.
// more specifically, all the constant coefficients come first, then the ones
// of *X*, then *X^2*, and so forth.
// this is where interleaving comes in handy! but let's take an example to
// understand the algorithm below.
//
// ## Example
// let's say we have 12 elements, namely *(e_0, e_1, ..., e_11)*, and we want to
// use a *(4, n)* code.
// we will then have 3 polynomials with 4 coefficients each:
// - *P_0 = e_0 + e_3 X + e_6 X^2 + e_9  X^3 = [e_0, e_3, e_6, e_9]*
// - *P_1 = e_1 + e_4 X + e_7 X^2 + e_10 X^3 = [e_1, e_4, e_7, e_10]*
// - *P_2 = e_2 + e_5 X + e_8 X^2 + e_11 X^3 = [e_2, e_5, e_8, e_11]*
//
// we can see that in each polynomial, the indices on *e_j* satisfy:
//     *j % 3 == i*
//   where *i* is the index of the polynomial.
//
// and we have:
// - *P_0*: 0, 3, 6 and 9 all satifsy *j % 3 == 0*
// - *P_1*: 1, 4, 7 and 10 all satifsy *j % 3 == 1*
// - *P_2*: 2, 5, 8 and 11 all satifsy *j % 3 == 2*
pub(crate) fn build_interleaved_polynomials<E, P>(
    elements: &[E::ScalarField],
    nb_polynomials: usize,
) -> Vec<P>
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    assert!(
        elements.len() % nb_polynomials == 0,
        "padding_not_supported: vector of elements ({}) should be divisible by the desired number of polynomials ({})",
        elements.len(),
        nb_polynomials
    );

    let mut polynomials = Vec::new();
    for i in 0..nb_polynomials {
        let coefficients = elements
            .iter()
            .enumerate()
            .filter(|(j, _)| j % nb_polynomials == i)
            .map(|(_, v)| *v)
            .collect::<Vec<_>>();
        polynomials.push(P::from_coefficients_vec(coefficients));
    }

    polynomials
}

#[cfg(test)]
mod tests {
    use std::ops::Div;

    use ark_bls12_381::Bls12_381;
    use ark_ec::pairing::Pairing;
    use ark_ff::PrimeField;
    use ark_poly::univariate::DensePolynomial;
    use ark_poly::DenseUVPolynomial;
    use ark_std::test_rng;
    use ark_std::UniformRand;

    use crate::field;

    type UniPoly381 = DensePolynomial<<Bls12_381 as Pairing>::ScalarField>;

    fn bytes() -> Vec<u8> {
        include_bytes!("../../res/dragoon_32x32.png").to_vec()
    }

    fn split_data_template<E: Pairing>(bytes: &[u8], modulus: usize, exact_length: Option<usize>) {
        let elements = field::split_data_into_field_elements::<E>(bytes, modulus);
        assert!(
            elements.len() % modulus == 0,
            "number of elements should be divisible by {}, found {}",
            modulus,
            elements.len()
        );

        if let Some(length) = exact_length {
            assert!(
                elements.len() == length,
                "number of elements should be exactly {}, found {}",
                length,
                elements.len()
            );
        }
    }

    #[test]
    fn split_data() {
        split_data_template::<Bls12_381>(&bytes(), 1, None);
        split_data_template::<Bls12_381>(&bytes(), 8, None);
        split_data_template::<Bls12_381>(&[], 1, None);
        split_data_template::<Bls12_381>(&[], 8, None);

        let nb_bytes = 11 * (<Bls12_381 as Pairing>::ScalarField::MODULUS_BIT_SIZE as usize / 8);
        split_data_template::<Bls12_381>(&bytes()[..nb_bytes], 1, Some(11));
        split_data_template::<Bls12_381>(&bytes()[..nb_bytes], 8, Some(16));
    }

    fn build_interleaved_polynomials_template<E, P>()
    where
        E: Pairing,
        P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        let rng = &mut test_rng();

        let elements = (0..12)
            .map(|_| E::ScalarField::rand(rng))
            .collect::<Vec<_>>();

        let actual = field::build_interleaved_polynomials::<E, P>(&elements, 3);
        let expected = vec![
            P::from_coefficients_vec(vec![elements[0], elements[3], elements[6], elements[9]]),
            P::from_coefficients_vec(vec![elements[1], elements[4], elements[7], elements[10]]),
            P::from_coefficients_vec(vec![elements[2], elements[5], elements[8], elements[11]]),
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn build_interleaved_polynomials() {
        build_interleaved_polynomials_template::<Bls12_381, UniPoly381>()
    }
}
