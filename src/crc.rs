use crc::{Crc, Algorithm};

// ECMA-167 CRC-16 algorithm parameters
// ECMA-167 7.2.6 Descriptor CRC
// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=44
// UDF 6.5 CRC Calculation
// http://www.osta.org/specs/pdf/udf260.pdf#page=118
const UDF_CRC_ALGO: Algorithm<u16> = Algorithm {
    width: 16,
    // x^16 + x^12 + x^5 + 1
    poly: 0x1021,
    init: 0x0000,
    refin: false,
    refout: false,
    xorout: 0x0000,
    check: 0x29b1,
    residue: 0x0000,
};

// Create a static CRC calculator with UDF parameters
const UDF_CRC: Crc<u16> = Crc::<u16>::new(&UDF_CRC_ALGO);

/// Calculate CRC-16 for UDF descriptor aka CRC-16/CCITT_FALSE
pub fn cksum(data: &[u8]) -> u16 {
    UDF_CRC.checksum(data)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_example() {
        // example from https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=46
        // “As an example, the CRC of the three bytes #70 #6A #77 is #3299”
        let crc = cksum(&[0x70, 0x6a, 0x77]);
        assert_eq!(crc, 0x3299);
    }
}
