/* -*- P4_16 -*- */


control calc_hash(
    in    bit<32>       ip,
    in    bit<16>       port,
    out   bit<16>       hash)

    (CRCPolynomial<bit<32>>       poly)
{
    Hash<bit<16>>(HashAlgorithm_t.CUSTOM, poly) hash_algo;

    action do_hash() {
        hash = hash_algo.get({
                ip,
                port
            });
    }

    apply {
        do_hash();
    }
}