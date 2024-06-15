#ifndef _PORT_MIRROR_
#define _PORT_MIRROR_

/***********************
* TypeDefs and Constants
************************/

typedef bit<4> internal_hdr_type_t;
typedef bit<4> internal_hdr_info_t;

const internal_hdr_type_t INTERNAL_HDR_TYPE_BRIDGED_META = 0xA;
const internal_hdr_type_t INTERNAL_HDR_TYPE_PORT_MIRROR = 0xB;


const internal_hdr_info_t INTERNAL_HDR_INFO_IG_PORT_MIRROR = 0x0;
const internal_hdr_info_t INTERNAL_HDR_INFO_EG_PORT_MIRROR = 0x1;

/* Mirror Types */
const int SIMPLE_PORT_MIRROR = 1;


/**********
* Headers
***********/

#define INTERNAL_HEADER           \
    internal_hdr_type_t type; \
    internal_hdr_info_t info

header internal_hdr_h {
    INTERNAL_HEADER;
}

header bridged_meta_h {
    INTERNAL_HEADER;
}

header port_mirror_h { // 7 bytes
    INTERNAL_HEADER;
    bit<48> timestamp;
}

struct port_mirror_meta_t{
    bridged_meta_h bridged_meta;
    port_mirror_h port_mirror;
    
    MirrorId_t mirror_session;
    internal_hdr_type_t internal_hdr_type;
    internal_hdr_info_t internal_hdr_info;
    bit<48> mirror_tstamp;
}

/**********
* Parser
***********/

parser PortMirroringEgressParser(
	packet_in pkt,
	out port_mirror_meta_t port_mirror_meta){

	internal_hdr_h internal_hdr;

	state start {
		internal_hdr = pkt.lookahead<internal_hdr_h>();
        transition select(internal_hdr.type, internal_hdr.info){
            (INTERNAL_HDR_TYPE_BRIDGED_META, _): parse_bridged_meta;
            (INTERNAL_HDR_TYPE_PORT_MIRROR, _): parse_port_mirror;
        }
	}

	state parse_bridged_meta {
        pkt.extract(port_mirror_meta.bridged_meta);
        transition accept;
    }

    state parse_port_mirror {
        pkt.extract(port_mirror_meta.port_mirror);
        transition accept;
    }

}


/**********
* Deparser
***********/
control PortMirroringCommonDeparser(
    in port_mirror_meta_t port_mirror_meta,
	in bit<3> mirror_type){

    Mirror() mirror; 

    apply {
        if(mirror_type == SIMPLE_PORT_MIRROR){ // eg_intr_md_for_dprsr.mirror_type
            mirror.emit<port_mirror_h>(port_mirror_meta.mirror_session, 
			{port_mirror_meta.internal_hdr_type, 
			port_mirror_meta.internal_hdr_info, 
			port_mirror_meta.mirror_tstamp}
			);
        }
    }
}


/*****************
* Ingress Control
******************/

control PortMirroringIngress(
	inout bridged_meta_h bridged_meta_hdr,
    inout port_mirror_meta_t port_mirror_meta,
    in PortId_t ingress_port,
	in bit<48> ingress_mac_tstamp,
    inout bit<3> ig_intr_md_for_dprsr_mirror_type){

    action _mirror_nop(){

	}

	action do_ingress_mirror(MirrorId_t ig_mirror_sess_id){
		ig_intr_md_for_dprsr_mirror_type = SIMPLE_PORT_MIRROR;
		port_mirror_meta.mirror_session = ig_mirror_sess_id;
		port_mirror_meta.internal_hdr_type = INTERNAL_HDR_TYPE_PORT_MIRROR;
		port_mirror_meta.internal_hdr_info = INTERNAL_HDR_INFO_IG_PORT_MIRROR;

		// Add ingress MAC timestamp
		port_mirror_meta.mirror_tstamp = ingress_mac_tstamp;
		
		// TODO: change dst_addr to the mirror server's NIC? 
	}

	table ingress_mirror {
		key = {
			ingress_port: exact;
		}

		actions = {
			do_ingress_mirror;
			@defaultonly _mirror_nop;
		}

		const default_action = _mirror_nop();

		size = 256;
	}

	action add_bridged_metadata(){
		bridged_meta_hdr.setValid();
        bridged_meta_hdr.type = INTERNAL_HDR_TYPE_BRIDGED_META;
        bridged_meta_hdr.info = 0;
	}

    apply{
        ingress_mirror.apply(); // for configured ports

		add_bridged_metadata(); // for ALL packets
    }

}


/*****************
* Egress Control
******************/

control PortMirroringEgress(
	inout bit<48> hdr_ethernet_src,
    inout port_mirror_meta_t port_mirror_meta,
	in PortId_t egress_port,
	in bit<48> eg_global_tstamp,
    inout bit<3> eg_intr_md_for_dprsr_mirror_type){

    action _mirror_nop(){

	}

	action do_egress_mirror(MirrorId_t eg_mirror_sess_id){
		eg_intr_md_for_dprsr_mirror_type = SIMPLE_PORT_MIRROR;
		port_mirror_meta.mirror_session = eg_mirror_sess_id;
		port_mirror_meta.internal_hdr_type = INTERNAL_HDR_TYPE_PORT_MIRROR;
		port_mirror_meta.internal_hdr_info = INTERNAL_HDR_INFO_EG_PORT_MIRROR;

		// Add ingress MAC timestamp
		port_mirror_meta.mirror_tstamp = eg_global_tstamp;

		// TODO: change dst_addr to the mirror server's NIC? 
	}

	table egress_mirror {
		key = {
			egress_port: exact;
		}

		actions = {
			do_egress_mirror;
			@defaultonly _mirror_nop;
		}

		const default_action = _mirror_nop();

		size = 256;
	}

	action copy_mirror_tstamp_to_packet(){
		hdr_ethernet_src = port_mirror_meta.port_mirror.timestamp;
	}

    apply {

		if (port_mirror_meta.port_mirror.isValid()){ // mirrored packet
			copy_mirror_tstamp_to_packet();
		}
		else if (port_mirror_meta.bridged_meta.isValid()){ // normal packet

			// indicate to mirror the packet
			// assign the mirroring session
			// also, assign the eg tstamp to meta
        	egress_mirror.apply();
			
		}
    }
}



#endif 