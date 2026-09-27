/// Integer-only decimal certificates, shared by unit and installed tests.
pub fn dyadic(numerator: u128, denominator_power: usize) -> String {
    let mut digits: Vec<u8> = numerator
        .to_string()
        .bytes()
        .rev()
        .map(|b| b - b'0')
        .collect();
    for _ in 0..denominator_power {
        let mut carry = 0;
        for digit in &mut digits {
            let next = *digit * 5 + carry;
            *digit = next % 10;
            carry = next / 10;
        }
        if carry != 0 {
            digits.push(carry);
        }
    }
    digits.resize(digits.len().max(denominator_power + 1), 0);
    let mut text = String::with_capacity(digits.len() + 1);
    for (index, digit) in digits.iter().enumerate().rev() {
        text.push(char::from(b'0' + digit));
        if index == denominator_power && denominator_power != 0 {
            text.push('.');
        }
    }
    text
}
