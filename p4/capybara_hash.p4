/* -*- P4_16 -*- */


control calc_hash(
    in    my_ingress_headers_t   hdr,
    in    my_ingress_metadata_t  meta,
    out   bit<16>                hash)

    (CRCPolynomial<bit<32>>       poly)
{
    Hash<bit<16>>(HashAlgorithm_t.CUSTOM, poly) hash_algo;

    action do_hash() {
        hash = hash_algo.get({
                meta.ip,
                meta.port
                // hdr.tcp_migration_header.client_ip,
                // hdr.tcp_migration_header.client_port
            });
    }

    apply {
        do_hash();
    }
}