# swarm.nu

This is used to managed multiple nodes at once (but it can be used to create a single node).

To see the help on a command, you can do either:
- `help swarm COMMAND`
- `swarm COMMAND --help`

## Swarm create

The only required parameter is `n` the number of nodes to create. `n` should be greater or equal to 1

Makes a table listing:
- the node index
- the user: local, or the name of the user account in case of ssh
- the ip_port: 127.0.0.1:3000 for local, or 192.168.33.210:3000 with ssh for example
- seed: the seed used to create the node keypair
- multiaddr: the node multiaddr
- storage: the amount of storage
- unit: the unit related to the amount of storage (K, M, G, etc.)

## Swarm run

Run the network, using a provided table (generally the one made by `swarm create`)

## Swarm list

List the nodes of the swarm that have been started with `swarm run`.

## Swarm kill

Kills all the spawned dragoonfly process. This also works over ssh, however you might be prompted for password to complete the ssh operation; unless you use an ssh keypair.

# network_builder.nu

Given a list of connections, creates the network, launches the nodes, make them listen on their respective http port and dials them to create the topology.

See documentation available inside [network_builder.nu](../cli/network_builder.nu)