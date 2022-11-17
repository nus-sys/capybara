Register< bit<32>, bit<16> >(1 << 16) reg0_request_client_ip; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_request_client_ip) write_reg0_request_client_ip = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_collision){
        if(register_data == 0){
            register_data = hdr.tcp_migration_header.client_ip;
            is_collision = 0;
        }else{
            is_collision = 1;
        }
    }
};
action exec_write_reg0_request_client_ip(bit<16> idx) {
    meta.is_collision = write_reg0_request_client_ip.execute(idx);
}

RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_request_client_ip) check_reg0_request_client_ip = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_matched){
        if(register_data == hdr.ipv4.src_ip){
            is_matched = 1;
        }else{
            is_matched = 0;
        }
    }
};
action exec_check_reg0_request_client_ip(bit<16> idx) {
    meta.request_ip_matched = check_reg0_request_client_ip.execute(idx);
}


Register< bit<16>, bit<16> >(1 << 16) reg0_request_client_port; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<16>, bit<16>, bit<1> >(reg0_request_client_port) write_reg0_request_client_port = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<1> is_collision){
        if(register_data == 0){
            register_data = hdr.tcp_migration_header.client_port;
            is_collision = 0;
        }else{
            is_collision = 1;
        }
    }
};
action exec_write_reg0_request_client_port() {
    write_reg0_request_client_port.execute(meta.hash_digest1);
}

RegisterAction< bit<16>, bit<16>, bit<1> >(reg0_request_client_port) check_reg0_request_client_port = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<1> is_matched){
        if(register_data == hdr.tcp.src_port){
            is_matched = 1;
        }else{
            is_matched = 0;
        }
    }
};
action exec_check_reg0_request_client_port(bit<16> idx) {
    meta.request_port_matched = check_reg0_request_client_port.execute(idx);
}

Register< bit<32>, bit<16> >(1 << 16) reg0_reply_client_ip; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_reply_client_ip) write_reg0_reply_client_ip = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_collision){
        register_data = hdr.tcp_migration_header.client_ip;
    }
};
action exec_write_reg0_reply_client_ip() {
    write_reg0_reply_client_ip.execute(meta.hash_digest1);
}


RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_reply_client_ip) check_reg0_reply_client_ip = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_matched){
        if(register_data == hdr.ipv4.src_ip){
            is_matched = 1;
        }else{
            is_matched = 0;
        }
    }
};
action exec_check_reg0_reply_client_ip(bit<16> idx) {
    meta.reply_ip_matched = check_reg0_reply_client_ip.execute(idx);
}

Register< bit<16>, bit<16> >(1 << 16) reg0_reply_client_port; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<16>, bit<16>, bit<1> >(reg0_reply_client_port) write_reg0_reply_client_port = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<1> is_collision){
        if(register_data == 0){
            register_data = hdr.tcp_migration_header.client_port;
            is_collision = 0;
        }else{
            is_collision = 1;
        }
    }
};
action exec_write_reg0_reply_client_port() {
    write_reg0_reply_client_port.execute(meta.hash_digest1);
}

RegisterAction< bit<16>, bit<16>, bit<1> >(reg0_reply_client_port) check_reg0_reply_client_port = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<1> is_matched){
        if(register_data == hdr.tcp.src_port){
            is_matched = 1;
        }else{
            is_matched = 0;
        }
    }
};
action exec_check_reg0_reply_client_port(bit<16> idx) {
    meta.reply_port_matched = check_reg0_reply_client_port.execute(idx);
}


Register< bit<32>, bit<16> >(1 << 16) reg0_target_mac_hi32; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_target_mac_hi32) write_reg0_target_mac_hi32 = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_collision){
        register_data = hdr.ethernet.src_mac[47:16];
    }
};
action exec_write_reg0_target_mac_hi32() {
    write_reg0_target_mac_hi32.execute(meta.hash_digest1);
}

RegisterAction< bit<32>, bit<16>, bit<32> >(reg0_target_mac_hi32) read_reg0_target_mac_hi32 = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<32> return_data){
        return_data = register_data;
    }
};
action exec_read_reg0_target_mac_hi32() {
    hdr.ethernet.dst_mac[47:16] = read_reg0_target_mac_hi32.execute(meta.hash_digest1);
}

Register< bit<32>, bit<16> >(1 << 16) reg0_origin_mac_hi32; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_origin_mac_hi32) write_reg0_origin_mac_hi32 = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_collision){
        register_data = hdr.ethernet.dst_mac[47:16];
    }
};
action exec_write_reg0_origin_mac_hi32() {
    write_reg0_origin_mac_hi32.execute(meta.hash_digest1);
}

RegisterAction< bit<32>, bit<16>, bit<32> >(reg0_origin_mac_hi32) read_reg0_origin_mac_hi32 = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<32> return_data){
        return_data = register_data;
    }
};
action exec_read_reg0_origin_mac_hi32() {
    hdr.ethernet.src_mac[47:16] = read_reg0_origin_mac_hi32.execute(meta.hash_digest2);
}



Register< bit<16>, bit<16> >(1 << 16) reg0_target_mac_lo16; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<16>, bit<16>, bit<1> >(reg0_target_mac_lo16) write_reg0_target_mac_lo16 = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<1> is_collision){
        register_data = hdr.ethernet.src_mac[15:0];
    }
};
action exec_write_reg0_target_mac_lo16() {
    write_reg0_target_mac_lo16.execute(meta.hash_digest1);
}

RegisterAction< bit<16>, bit<16>, bit<16> >(reg0_target_mac_lo16) read_reg0_target_mac_lo16 = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<16> return_data){
        return_data = register_data;
    }
};
action exec_read_reg0_target_mac_lo16() {
    hdr.ethernet.dst_mac[15:0] = read_reg0_target_mac_lo16.execute(meta.hash_digest1);
}

Register< bit<16>, bit<16> >(1 << 16) reg0_origin_mac_lo16; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<16>, bit<16>, bit<1> >(reg0_origin_mac_lo16) write_reg0_origin_mac_lo16 = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<1> is_collision){
        register_data = hdr.ethernet.dst_mac[15:0];
    }
};
action exec_write_reg0_origin_mac_lo16() {
    write_reg0_origin_mac_lo16.execute(meta.hash_digest1);
}

RegisterAction< bit<16>, bit<16>, bit<16> >(reg0_origin_mac_lo16) read_reg0_origin_mac_lo16 = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<16> return_data){
        return_data = register_data;
    }
};
action exec_read_reg0_origin_mac_lo16() {
    hdr.ethernet.src_mac[15:0] = read_reg0_origin_mac_lo16.execute(meta.hash_digest2);
}


Register< bit<32>, bit<16> >(1 << 16) reg0_target_ip; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_target_ip) write_reg0_target_ip= { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_collision){
        register_data = hdr.tcp_migration_header.target_ip;
    }
};
action exec_write_reg0_target_ip() {
    write_reg0_target_ip.execute(meta.hash_digest1);
}

RegisterAction< bit<32>, bit<16>, bit<32> >(reg0_target_ip) read_reg0_target_ip = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<32> return_data){
        return_data = register_data;
    }
};
action exec_read_reg0_target_ip() {
    hdr.ipv4.dst_ip = read_reg0_target_ip.execute(meta.hash_digest1);
}

Register< bit<32>, bit<16> >(1 << 16) reg0_origin_ip; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<32>, bit<16>, bit<1> >(reg0_origin_ip) write_reg0_origin_ip= { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<1> is_collision){
        register_data = hdr.tcp_migration_header.origin_ip;
    }
};
action exec_write_reg0_origin_ip() {
    write_reg0_origin_ip.execute(meta.hash_digest1);
}

RegisterAction< bit<32>, bit<16>, bit<32> >(reg0_origin_ip) read_reg0_origin_ip = { // <value, index, out>
    void apply(inout bit<32> register_data, out bit<32> return_data){
        return_data = register_data;
    }
};
action exec_read_reg0_origin_ip() {
    hdr.ipv4.src_ip = read_reg0_origin_ip.execute(meta.hash_digest2);
}


Register< bit<16>, bit<16> >(1 << 16) reg0_target_port; // <value, index> => 2^16 = 65536 entries
RegisterAction< bit<16>, bit<16>, bit<1> >(reg0_target_port) write_reg0_target_port = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<1> is_collision){
        register_data = hdr.tcp_migration_header.target_port;
    }
};
action exec_write_reg0_target_port() {
    write_reg0_target_port.execute(meta.hash_digest1);
}

RegisterAction< bit<16>, bit<16>, bit<16> >(reg0_target_port) read_reg0_target_port = { // <value, index, out>
    void apply(inout bit<16> register_data, out bit<16> return_data){
        return_data = register_data;
    }
};
action exec_read_reg0_target_port() {
    hdr.tcp.dst_port = read_reg0_target_port.execute(meta.hash_digest1);
}

table tb_write_reply_client_ip {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_reply_client_ip;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_reply_client_ip();
    }
}

table tb_write_request_client_port {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_request_client_port;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_request_client_port();
    }
}

table tb_write_reply_client_port {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_reply_client_port;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_reply_client_port();
    }
}

table tb_write_target_mac_hi32 {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_target_mac_hi32;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_target_mac_hi32();
    }
}

table tb_write_origin_mac_hi32 {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_origin_mac_hi32;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_origin_mac_hi32();
    }
}

table tb_write_target_mac_lo16 {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_target_mac_lo16;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_target_mac_lo16();
    }
}

table tb_write_origin_mac_lo16 {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_origin_mac_lo16;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_origin_mac_lo16();
    }
}

table tb_write_target_ip {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_target_ip;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_target_ip();
    }
}

table tb_write_origin_ip {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_origin_ip;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_origin_ip();
    }
}

table tb_write_target_port {
    key = {
        meta.is_collision : exact;
    }
    actions = {
        exec_write_reg0_target_port;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        0 : exec_write_reg0_target_port();
    }
}




table tb_request_mac_hi32 {
    key = {
        meta.request_ip_matched     : exact;
        meta.request_port_matched   : exact;
        meta.reply_ip_matched       : exact;
        meta.reply_port_matched     : exact;
    }
    actions = {
        exec_read_reg0_target_mac_hi32;
        // exec_read_reg0_origin_mac_hi32;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        (1, 1, 0, 0) : exec_read_reg0_target_mac_hi32();
        // (0, 0, 1, 1) : exec_read_reg0_origin_mac_hi32();   
    }
}

table tb_request_mac_lo16 {
    key = {
        meta.request_ip_matched : exact;
        meta.request_port_matched : exact;
    }
    actions = {
        exec_read_reg0_target_mac_lo16;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        (1, 1) : exec_read_reg0_target_mac_lo16();
    }
}

table tb_request_ip {
    key = {
        meta.request_ip_matched : exact;
        meta.request_port_matched : exact;
    }
    actions = {
        exec_read_reg0_target_ip;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        (1, 1) : exec_read_reg0_target_ip();
    }
}

table tb_request_port {
    key = {
        meta.request_ip_matched : exact;
        meta.request_port_matched : exact;
    }
    actions = {
        exec_read_reg0_target_port;
        @defaultonly NoAction;
    }
    size = 16;
    const default_action = NoAction();
    const entries = {
        (1, 1) : exec_read_reg0_target_port();
    }
}