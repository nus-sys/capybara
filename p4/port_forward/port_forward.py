# manufactured base on cluster hardware sheet 2022.12.04
if 0:
    bfrt = ...
port = bfrt.port.port
port.clear()
for entry in bfrt.pre.node.dump(return_ents=True) or []:
    entry.remove()
for entry in bfrt.pre.mgid.dump(return_ents=True) or []:
    entry.remove()
ingress = bfrt.port_forward.pipe.Ingress
ingress.l2_forwarding_decision.clear()

routes = (
    (0xb8cef62a2f94, 0, '100G'),
    (0xb8cef62a45fc, 4, '100G'),
    (0xb8cef62a3f9c, 8, '100G'),
    (0xb8cef62a30ec, 12, '100G'),
    (0x1070fdc89474, 16, '40G'),
    (0x1070fdc8944c, 20, '40G'),
    (0x08c0ebb6cd5c, 24, '100G'),
    (0x08c0ebb6e804, 28, '100G'),
    (0x08c0ebb6c5ac, 32, '100G'),
    (0x08c0ebb6e7e4, 36, '100G'),
    #
)
for _, dev_port, speed in routes:
    port.add(
        DEV_PORT=dev_port, SPEED=f'BF_SPEED_{speed}', FEC='BF_FEC_TYP_NONE',
        PORT_ENABLE=True, AUTO_NEGOTIATION='PM_AN_FORCE_DISABLE')
port.add(
    DEV_PORT=40, SPEED='BF_SPEED_100G', FEC='BF_FEC_TYP_NONE',
    PORT_ENABLE=True, AUTO_NEGOTIATION='PM_AN_FORCE_DISABLE')

ingress.l2_forwarding_decision.add_with_broadcast(dst_addr=0xffffffffffff)

for addr, port, _ in routes:
    ingress.l2_forwarding_decision.add_with_l2_forward(dst_addr=addr, port=port)
    bfrt.pre.node.add(port, port, [], [port])
    bfrt.pre.prune.mod(port, [port])
bfrt.pre.mgid.add(
    1,
    [port for _, port, _ in routes],
    [False for _ in routes],
    [0 for _ in routes])