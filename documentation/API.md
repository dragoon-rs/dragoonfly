# The commands

If any parameter in the URL path of a GET request contains a `/`, it should be URL-encoded to `%2F`. Lists should be encoded using `JSON` array format, without URL encoding if they are in a POST request.

- [Listen](#listen-get)
- [Dial single](#dial-single-post)
- [Dial multiple](#dial-multiple-post)
- [Encode file](#encode-file-post)
- [Start provide](#start-provide-post)
- [Stop provide](#stop-provide-post)
- [Get providers](#get-providers-post)
- [Get blocks info from](#get-blocks-info-from-get)
- [Get block list](#get-block-list-get)
- [Get block from](#get-block-from-get)
- [Decode blocks](#decode-blocks-post)
- [Get file](#get-file-get)
- [Node info](#node-info-get)
- [Get connected peers](#get-connected-peers-get)
- [Send block to](#send-block-to-post)
- [Send block list](#send-block-list-post)
- [Get available send storage](#get-available-send-storage-get)
- [Change available send storage](#change-available-send-storage-post)

## Note

All the `cURL` examples do exactly the same thing as the `Nushell` examples.

### Listen (GET)

Ask the node to listen on its http interface, making it available to communicate with other nodes of the network.

*Query route*

```
listen/MULTIADDR
```

*Parameters:*

- `MULTIADDR`: the multi-address the node will attempt to listen on. The node should be authorized to listen on all the ip+port of the multiaddr and there shouldn't be another process on it.

*Return*:

Returns `1` if the dial succeeds, otherwise an error.


__Nushell example__:

```
dragoon listen --node 127.0.0.1:3000 /ip4/127.0.0.1/tcp/31200
```

Will ask the node on `127.0.0.1:3000` to listen on `127.0.0.1` and to use the port `31200` for TCP communications


__cURL example__:

```
curl http://127.0.0.1:3000/listen/%2Fip4%2F127.0.0.1%2Ftcp%2F31200
```

### Dial single (POST)

Try to connect to another node (like ringing its phone basically).

*Query route*

```
dial-single/
```

*Post body:*

- `MULTIADDR`: the multi-address the node will dial. This only works if:
   - the receiving node has used a `listen` on the multiaddr they are dialed on
   - the node starting the dial has used `listen` on a ip+port which accepts the same protocol as the receiving node (for example, TCP or UDP)

*Return*:

Nothing if it succeeds, otherwise an error

__Nushell example__:

```
dragoon dial-single --node 127.0.0.1:3000 /ip4/127.0.0.1/tcp/31201
```

Will ask the node on `127.0.0.1:3000` to dial another node on `127.0.0.1:31201` using TCP.


__cURL example__:

```
curl -X POST "http://127.0.0.1:3000/dial-single" -H "Content-Type: Application/Json" -d '"/ip4/127.0.0.1/tcp/31201"'
```

### Dial multiple (POST)

Same as dial-single but will try to connect to several nodes at the same time. This is faster than sequentially executing the equivalent dial-single commands.

*Query route*

```
dial-multiple/
```

*Post body:*

- `LIST<MULTIADDR>`: the multi-addresses the node will try to dial. This only works if:
   - each receiving node has used a `listen` on the multiaddr they are dialed on
   - the node starting the dial has used `listen` on a ip+port which accepts the same protocol as the receiving node (for example, TCP or UDP). Note that not all the receiving nodes need to use the same protocol. If the dialing node has a port for TCP and one for UDP, then it doesn't matter if the node being dialed uses TCP or UDP

*Return*:

Nothing if it succeeds, otherwise an error

__Nushell example__:

```
dragoon dial-multiple --node 127.0.0.1:3000 [/ip4/127.0.0.1/tcp/31201, /ip4/127.0.0.1/tcp/31202]
```

Will ask the node on `127.0.0.1:3000` to dial other nodes on `/ip4/127.0.0.1/tcp/31201` and `/ip4/127.0.0.1/tcp/31202` using TCP


__cURL example__:

```
curl -X POST "http://127.0.0.1:3000/dial-multiple" -H "Content-Type: Application/Json" -d '["/ip4/127.0.0.1/tcp/31201", "/ip4/127.0.0.1/tcp/31202"]'
```

### Encode file (POST)

Using Komodo, encode a file into multiple blocks of data.

*Query route*

```
encode-file
```

*Post body:*
- `FILE_PATH`: the path to the file the node will encode
- `REPLACE_BLOCKS`: if blocks already exist for this file, should they be deleted before encoding the file into new blocks
- `k`: minimal number of block required to decode the file
- `n`: how many blocks to produce. `k` needs to be smaller than `n`
- `ENCODING_METHOD`: when making the encoding matrix, how should the coefficients be chosen:
   - Random
   - Vandermonde

*Return*:

```
╭───┬───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ 0 │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e                                                                                                                                              │
│ 1 │ ["2bc87095956bcdca4e2cdfc37d53f2cebc7e494642523bb618b269183cd82b5","eb10d8f286825fc7b677d8b41b84f0225268fe45a42d21a8b53cd7d7ef54b6","1ca8eb0822eb9b7b438b05c8f3320199b526d4080b0d6a11d6bc1a92fac866","cbfccfa │
│   │ 69e4d9ee97fd51f253672d1bc8089f38c72acc81cefc5a9d977edf","3b3a10b3a36a684aedb31a5f9b162243813048d8234e9c37a3d28fa8c4414d50",]                                                                                  │
╰───┴───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```
- the hash of the file
- the list of block hashes

__Nushell example__:

```
dragoon encode-file --node 127.0.0.1:3000 --k 2 --n 7 --replace-blocks --encoding-method Vandermonde tests/assets/dragoon_32/dragoon_32x32.png 
```

Will encode the file at `tests/assets/dragoon_32/dragoon_32x32.png`, replacing the blocks if they exist, using a Vandermonde matrix for encoding. 2 blocks will be required to decode the file, 7 blocks in total will be made.

__cURL example__:

```
curl -X POST "http://127.0.0.1:3000/encode-file" -H "Content-Type: Application/Json" -d '["tests/assets/dragoon_32/dragoon_32x32.png", true, "Vandermonde", 5, 7]'
```

### Start provide (POST)

Announce through the hash of the file that a node has some blocks of this file to peers of the network, and that it can share those blocks.

*Query route*

```
start-provide/
```

*Post body*:
- `FILE_HASH`: the hash of the file the node wants to start provide

*Failure case*

To start providing, the node that provides the file needs a minimum number of nodes it can give this information to. The default is 1. If the node doesn't know any other node, the provide will fail.

__Nushell example__:

```
dragoon start-provide --node 127.0.0.1:3000 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e
```

Will ask the node on `127.0.0.1:3000` to provide the file `79c...35e`

__cURL Example__:

```
curl -X POST "http://127.0.0.1:3000/start-provide" -H "Content-Type: Application/Json" -d '"79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e"'
```

### Stop provide (POST)

Local operation which removes the record corresponding to the hash of the file from the re-publication list.
When then expiry time of the record comes, it will not be sent again to other nodes. This means that until expiry, other nodes can still hold the record saying a node provides the file, even if this node used stop provide.

*Query route*

```
stop-provide/
```

*Post body*:
- `FILE_HASH`: the hash of the file the node wants to stop provide

__Nushell example__:

```
dragoon stop-provide --node 127.0.0.1:3000 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e
```

Will ask the node on `127.0.0.1:3000` to stop providing the file `79c...35e`

__cURL Example__:

```
curl -X POST "http://127.0.0.1:3000/stop-provide" -H "Content-Type: Application/Json" -d '"79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e"'
```

__Note__:

Currently, it is possible to ask a node for blocks even if this node didn't say it provides those blocks. It means until expiry of the record of the start-provide for a file, it is likely other nodes will still ask the node that provided the file to send the blocks, even if it used stop-provide.

### Get providers (POST)

The other side of start provide. Search in the network which peers have started to provide some blocks of the file the node is searching for. This is done using a Kademlia search.

*Query route*:

```
get-providers/
```

*Post body*:
- `FILE_HASH`: the hash of the file the node will search for

*Return*:
The list of peer ids corresponding to peers who announced they provide the `FILE_HASH`

__Nushell example__:

```
dragoon get-providers --node 127.0.0.1:3001 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e
```

Will ask the node on `127.0.0.1:3001` to search which peers provide the file `79c...35e`

*Return*:

```
╭───┬──────────────────────────────────────────────────────╮
│ 0 │ 12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN │
╰───┴──────────────────────────────────────────────────────╯
```

__cURL example__:

```
curl -X POST "http://127.0.0.1:3001/get-providers" -H "Content-Type: Application/Json" -d '"79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e"'
```

### Get blocks info from (GET)

Ask a peer that provides some blocks of a file to give a list that contains information about each block it provides for this file.

*Query route*:

```
get-blocks-info-from/PEER_ID/FILE_HASH
```

*Parameters*:
- `PEER_ID`: the peer id of the peer we will request the list of block info to
- `FILE_HASH`: the hash of the file whose block info we want

*Return*:
A list containing:
- the `PEER_ID` of the peer answering to the request (this return is used internally for other operations)
- the `FILE_HASH` (this return is used internally for other operations)
- a list of block info

*Note*:
Currently, the only information received about a block is its hash, but this could change in the future to include the size of the block or its creation date for example.

__Nushell example__:

```
dragoon get-blocks-info-from --node 127.0.0.1:3001 12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e
```

Will ask the peer `12D...XTN` to provide information about the file whose hash is `79c...35e`

It returns:

```
╭─────────────────┬──────────────────────────────────────────────────────────────────────────╮
│ peer_id_base_58 │ 12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN                     │
│ file_hash       │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e         │
│                 │ ╭───┬──────────────────────────────────────────────────────────────────╮ │
│ block_hashes    │ │ 0 │ 3b3a10b3a36a684aedb31a5f9b162243813048d8234e9c37a3d28fa8c4414d50 │ │
│                 │ │ 1 │ eb10d8f286825fc7b677d8b41b84f0225268fe45a42d21a8b53cd7d7ef54b6   │ │
│                 │ │ 2 │ 1ca8eb0822eb9b7b438b05c8f3320199b526d4080b0d6a11d6bc1a92fac866   │ │
│                 │ │ 3 │ cbfccfa69e4d9ee97fd51f253672d1bc8089f38c72acc81cefc5a9d977edf    │ │
│                 │ │ 4 │ 2bc87095956bcdca4e2cdfc37d53f2cebc7e494642523bb618b269183cd82b5  │ │
│                 │ ╰───┴──────────────────────────────────────────────────────────────────╯ │
╰─────────────────┴──────────────────────────────────────────────────────────────────────────╯
```

__cURL example__:

```
curl "http://127.0.0.1:3001/get-blocks-info-from/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN/79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e"
```

### Get block list (GET)

Local query to search which blocks of a file are on the disk. This is usually what a node does when another node asks it which blocks it provides for a given file.

*Query route*

```
get-block-list/FILE_HASH
```

*Parameters*
- `FILE_HASH`: the hash of the file for which we want the list of block hashes

*Returns*:
A list of block hashes

__Nushell example__:

```
dragoon get-block-list --node 127.0.0.1:3000 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e
```

Will ask the node on `127.0.0.1:3000` to search locally what blocks are part of `79c...35e`

It returns:
```
╭───┬──────────────────────────────────────────────────────────────────╮
│ 0 │ 3b3a10b3a36a684aedb31a5f9b162243813048d8234e9c37a3d28fa8c4414d50 │
│ 1 │ eb10d8f286825fc7b677d8b41b84f0225268fe45a42d21a8b53cd7d7ef54b6   │
│ 2 │ 1ca8eb0822eb9b7b438b05c8f3320199b526d4080b0d6a11d6bc1a92fac866   │
│ 3 │ cbfccfa69e4d9ee97fd51f253672d1bc8089f38c72acc81cefc5a9d977edf    │
│ 4 │ 2bc87095956bcdca4e2cdfc37d53f2cebc7e494642523bb618b269183cd82b5  │
╰───┴──────────────────────────────────────────────────────────────────╯
```

__cURL example__:

```
curl "http://127.0.0.1:3000/get-block-list/79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e"
```

The list of block hashes that the current node has (no network request)

### Get block from (GET)

Ask another peer to send the data of a given block (identified by the hash of the file and the hash of the block).

*Query route*:

```
get-block-from/PEER_ID/FILE_HASH/BLOCK_HASH/SAVE_BLOCK
```

*Parameters*:
- `PEER_ID`: the peer id of the peer to request the block to
- `FILE_HASH`: the hash of the file the block is part of
- `BLOCK_HASH`: the hash of the block that we want
- `SAVE_BLOCK`: boolean, if the node will save the block to disk or output the result

__Nushell example__:

```
dragoon get-block-from --node 127.0.0.1:3001 12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e b3a10b3a36a684aedb31a5f9b162243813048d8234e9c37a3d28fa8c4414d50
```

Will ask the node on `127.0.0.1:3001` to send a request to `12D...XTN` to get the block `b3a...d50` part of file `79c...35e` and will save it to disk (save is activated by default for the Nu interface)

__cURL example__:

```
curl "http://127.0.0.1:3001/get-block-from/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN/79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e/b3a10b3a36a684aedb31a5f9b162243813048d8234e9c37a3d28fa8c4414d50
```

### Decode blocks (POST)

Try to decode a file from a list of blocks. This can fail if:

- all blocks are not related to the same file
- there are not enough blocks to decode the file
- one or more blocks are corrupted
- there is a linear dependency between too many blocks

*Query route*:

```
decode-blocks/
```

*Post body*:
- `BLOCK_DIR`: the directory in which the blocks are
- `BLOCK_HASHES`: a list of the block hash, those blocks will be used to make the file
- `OUTPUT_FILENAME`: the filename of the decoded file

__Nushell example__:

```
dragoon decode-blocks --node 127.0.0.1:3001 /tmp/received_blocks/ [fb82767513fe66588234cc858614bafbbd9caf239c03ba4ccc9f3d3a0aa6134, c4aa66f9f66ca6df91c3ab9da9d7beedd84fdc239d5d6daa30a758d138adb, 72f645dddbd7e34b66e7c625c4650eed636c422c451a7fcb410777878f6885, b734a75158e0dee44049efa7876bc69ad33063385fb38e5df7edf529d7a9a63b] decoded_dragoon.png
```

Will use the blocks `[fb82767513fe66588234cc858614bafbbd9caf239c03ba4ccc9f3d3a0aa6134, c4aa66f9f66ca6df91c3ab9da9d7beedd84fdc239d5d6daa30a758d138adb, 72f645dddbd7e34b66e7c625c4650eed636c422c451a7fcb410777878f6885, b734a75158e0dee44049efa7876bc69ad33063385fb38e5df7edf529d7a9a63b]` in the directory `/tmp/received_blocks/` to decode the file and will write the result to `/tmp/decoded_dragoon.png` (parent of the `BLOCK_DIR`)

__cURL example__:

```
curl -X POST "http://127.0.0.1:3001/decode-blocks" -H "Content-Type: Application/Json" -d '["/tmp/received_blocks/", ["fb82767513fe66588234cc858614bafbbd9caf239c03ba4ccc9f3d3a0aa6134", "c4aa66f9f66ca6df91c3ab9da9d7beedd84fdc239d5d6daa30a758d138adb", "72f645dddbd7e34b66e7c625c4650eed636c422c451a7fcb410777878f6885", "b734a75158e0dee44049efa7876bc69ad33063385fb38e5df7edf529d7a9a63b"], "decoded_dragoon.png"]'
```

### Get file (GET)

Wrapper command that automatically performs the following commands:
- [Get providers](#get-providers)
- [Get blocks info from](#get-blocks-info-from)
- [Get block list](#get-block-list)
- [Get block from](#get-block-from)
- [Decode blocks](#decode-blocks)

*Query route*:

```
get-file/FILE_HASH/OUTPUT_FILENAME
```

*Parameters*:
- `FILE_HASH`: the hash of the file 
- `OUTPUT_FILENAME`: how the decoded file should be named

*Returns*:

The path where the file was saved

__Nushell example__:

```
dragoon get-file --node 127.0.0.1:3001 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e "hello_there"
```

Will ask the node on `127.0.0.1:3001` to get the file of hash `79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e` and to write the result in a file named `hello_there`

It returns:
`~/.share/dragoonfly/12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X/files/79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e/hello_there`
 

__cURL example__:

```
curl http://127.0.0.1:3001/get-file/79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e/hello_there
```

### Node info (GET)

Returns information about the current node. This is not a command used by a node to require information about another node, but as a user to get information about a node this user owns.

*Query route*:

```
node-info
```

*Returns*:

A list containing:
- the node peer id
- the node label (its name, if one was given to it when it was created)

__Nushell example__:

```
dragoon node-info --node 127.0.0.1:3000
```

It returns:

```
╭───┬──────────────────────────────────────────────────────╮
│ 0 │ 12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN │
│ 1 │ 12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN │
╰───┴──────────────────────────────────────────────────────╯
```

When no label is given, the label is the same as the peer ID

```
╭───┬──────────────────────────────────────────────────────╮
│ 0 │ 12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN │
│ 1 │ my_precious                                          │
╰───┴──────────────────────────────────────────────────────╯
```

Node was named `my_precious`

__cURL example__:

```
curl http://127.0.0.1:3000/node-info
```

### Get connected peers (GET)

Get the peer ids of all the nodes currently connected to a given node.

*Query route*

```
get-connected-peers
```

__Example__:

```
get-connected-peers
```

*Returns*:

A list of peer ID for nodes connected to the node we asked

__Nushell example__:

```
dragoon get-connected-peers --node 127.0.0.1:3000
```

It returns:
```
╭───┬──────────────────────────────────────────────────────╮
│ 0 │ 12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X │
│ 1 │ 12D3KooWH3uVF6wv47WnArKHk5p6cvgCJEb74UTmxztmQDc298L3 │
│ 2 │ 12D3KooWLJtG8fd2hkQzTn96MrLvThmnNQjTUFZwGEsLRz5EmSzc │
╰───┴──────────────────────────────────────────────────────╯
```
The list of peer id for nodes connected to the node on `127.0.0.1:3000`

__cURL example__:

```
curl http://127.0.0.1:3000/get-connected-peers
```

### Send block to (POST)

Sends a block to a given peer. We first ask this peer if they accept to receive the block.

*Query route*
```
send-block-to/
```

*Post body*:
- `PEER_ID`: the peer id of the peer we want to send the block to
- `FILE_HASH`: the hash of the file the block is part of
- `BLOCK_HASH`: the hash of the block we want to send

*Returns*

- A boolean to tell if it succeeded in sending the block or not
- A list containing:
    - the peer ID we sent to
    - the file hash
    - the block hash

The second part is not very useful to the user, but it is used in other command calls to keep track of the progress of certain operations.

*Failure case*:
- Cannot connect to the other peer
- The other peer refuses to receive the block (this is generally due to insufficient storage space)
- The other block verified the block and found that it wasn't valid, thus not storing it.
- Another protocol failure, this includes but is not limited to:
    - announced was block size was different from the size of the block that was sent
    - connection dropped by either end

__Nushell example__:

```
dragoon send-block-to --node 127.0.0.1:3000 12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e 7a66470e6e28ef17ea5e46d867bd9fdff39d262692587cab1b43ff4ed23c1
```

Will ask the node on `127.0.0.1:3000` to send the block `7a66470e6e28ef17ea5e46d867bd9fdff39d262692587cab1b43ff4ed23c1` part of file `79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e` to peer `12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X`

It returns:
```
╭───┬──────────────────────────────────────────────────────────────────────────╮
│ 0 │ true                                                                     │
│ 1 │ ╭───┬──────────────────────────────────────────────────────────────────╮ │
│   │ │ 0 │ 12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X             │ │
│   │ │ 1 │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e │ │
│   │ │ 2 │ 7a66470e6e28ef17ea5e46d867bd9fdff39d262692587cab1b43ff4ed23c1    │ │
│   │ ╰───┴──────────────────────────────────────────────────────────────────╯ │
╰───┴──────────────────────────────────────────────────────────────────────────╯
```

__cURL example__:

```
curl -X POST "http://127.0.0.1:3000/send-block-to" -H "Content-Type: Application/Json" -d '["12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X", "79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e", "7a66470e6e28ef17ea5e46d867bd9fdff39d262692587cab1b43ff4ed23c1"]'
```

### Send block list (POST)

Sends a list of blocks using a given strategy.

*Query route*:
```
send-block-list
```

*Post body*:
- `STRATEGY_NAME`: which strategy to use to choose who to send which block to, possible values:
    - `Random`: randomly choose a peer you know for each block
    - `RoundRobin`: list all the peer you know, send a block to each. If some are left, start again
- `FILE_HASH`: the hash of the file the blocks are part of
- `BLOCK_LIST`: list of block hashes, the blocks to send

*Returns*:

A list of list, each sub-list contains:
- the hash of the block
- the hash of file
- which peer this particular block was sent to

For all the blocks that were sent.

*Failure case*:

This fails if not all blocks could be sent. This can be because none of the connected peers have enough storage left to store new blocks.

__Nushell example__:

```
dragoon send-block-list --node 127.0.0.1:3000 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e [db8bd2629f7212b64a2a86c8db2d052512f7d1d61a7bf63ec7ec421fd2d477a, 96d3bbeb23cd613957ba8f5655a29c96428ac51b6638cc54da1aa52f5b23514, 10972bb9d3b59648c3ba445b4b572b5523ad465941ab756687fa89c157815be, dec3a4efeb49f53d1128a1958aabfcb4e177cca08d9adbdcb0c145bb88515a, 2e7d9baad8a3c89c6f3ebe721dee1af7d9e84c96c8693c1729a7c0e7a4a231] --strategy-name "RoundRobin"
```

It returns:
```
╭───┬──────────────────────────────────────────────────────────────────────────╮
│ 0 │ ╭───┬──────────────────────────────────────────────────────────────────╮ │
│   │ │ 0 │ 12D3KooWLJtG8fd2hkQzTn96MrLvThmnNQjTUFZwGEsLRz5EmSzc             │ │
│   │ │ 1 │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e │ │
│   │ │ 2 │ 96d3bbeb23cd613957ba8f5655a29c96428ac51b6638cc54da1aa52f5b23514  │ │
│   │ ╰───┴──────────────────────────────────────────────────────────────────╯ │
│ 1 │ ╭───┬──────────────────────────────────────────────────────────────────╮ │
│   │ │ 0 │ 12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X             │ │
│   │ │ 1 │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e │ │
│   │ │ 2 │ 10972bb9d3b59648c3ba445b4b572b5523ad465941ab756687fa89c157815be  │ │
│   │ ╰───┴──────────────────────────────────────────────────────────────────╯ │
│ 2 │ ╭───┬──────────────────────────────────────────────────────────────────╮ │
│   │ │ 0 │ 12D3KooWH3uVF6wv47WnArKHk5p6cvgCJEb74UTmxztmQDc298L3             │ │
│   │ │ 1 │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e │ │
│   │ │ 2 │ 2e7d9baad8a3c89c6f3ebe721dee1af7d9e84c96c8693c1729a7c0e7a4a231   │ │
│   │ ╰───┴──────────────────────────────────────────────────────────────────╯ │
│ 3 │ ╭───┬──────────────────────────────────────────────────────────────────╮ │
│   │ │ 0 │ 12D3KooWH3uVF6wv47WnArKHk5p6cvgCJEb74UTmxztmQDc298L3             │ │
│   │ │ 1 │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e │ │
│   │ │ 2 │ db8bd2629f7212b64a2a86c8db2d052512f7d1d61a7bf63ec7ec421fd2d477a  │ │
│   │ ╰───┴──────────────────────────────────────────────────────────────────╯ │
│ 4 │ ╭───┬──────────────────────────────────────────────────────────────────╮ │
│   │ │ 0 │ 12D3KooWQYhTNQdmr3ArTeUHRYzFg94BKyTkoWBDWez9kSCVe2Xo             │ │
│   │ │ 1 │ 79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e │ │
│   │ │ 2 │ dec3a4efeb49f53d1128a1958aabfcb4e177cca08d9adbdcb0c145bb88515a   │ │
│   │ ╰───┴──────────────────────────────────────────────────────────────────╯ │
╰───┴──────────────────────────────────────────────────────────────────────────╯
```

We can see that:
- Node `12D3KooWH3uVF6wv47WnArKHk5p6cvgCJEb74UTmxztmQDc298L3` received blocks `2e7d9baad8a3c89c6f3ebe721dee1af7d9e84c96c8693c1729a7c0e7a4a231` and `db8bd2629f7212b64a2a86c8db2d052512f7d1d61a7bf63ec7ec421fd2d477a`
- Node `db8bd2629f7212b64a2a86c8db2d052512f7d1d61a7bf63ec7ec421fd2d477a` received block `96d3bbeb23cd613957ba8f5655a29c96428ac51b6638cc54da1aa52f5b23514`
- etc.

__cURL example__:

```
curl -X POST "http://127.0.0.1:3000/send-block-list" -H "Content-Type: Application/Json" -d '["RoundRobin", "79c29b5bddd0ffa7af86cc4d8a46e9fb6a872faaaf96c3862799101c28bd135e", ["db8bd2629f7212b64a2a86c8db2d052512f7d1d61a7bf63ec7ec421fd2d477a", "96d3bbeb23cd613957ba8f5655a29c96428ac51b6638cc54da1aa52f5b23514", "10972bb9d3b59648c3ba445b4b572b5523ad465941ab756687fa89c157815be", "dec3a4efeb49f53d1128a1958aabfcb4e177cca08d9adbdcb0c145bb88515a", "2e7d9baad8a3c89c6f3ebe721dee1af7d9e84c96c8693c1729a7c0e7a4a231"]]'
```


### Get available send storage (GET)

Check how much storage space is left for blocks received through a send request.

*Query route*:

```
get-available-send-storage
```

*Returns*:

The size in bytes of how much storage is left

__Nushell example__:

```
dragoon get-available-send-storage --node 127.0.0.1:3002
```

It returns:

`19999999044`

Size in bytes left on disk (a bit under 20 GB) for the blocks received by sent request

__cURL example__:

```
curl http://127.0.0.1:3002/get-available-send-storage
```

### Change available send storage (POST)

Change the size that is attributed to a node for the quantity of blocks it can receive.

*Query route*:
```
change-available-send-storage
```

*Post body*:
- `NEW_STORAGE_SIZE`: the size you allow the node to use

*Returns*:

A string with some information about the size left on disk.

*Note*:

Changing this changes the __total__ available storage space. This means that the actual space left is the total minus what is already stored. A node that stored 9 GB of blocks and changes the new available storage space to 10 GB only has 1 GB left to receive new blocks by send request.

Also note the following:
- It is possible to put a size that is bigger than the actual physical size of the disk. This means the node will not stop receiving blocks until there is physically no space left on disk.
- It is possible to put a size that is smaller than the current total size of blocks on disk (for instance, 5 GB in the previous example). This means that the node will now refuse any new send request.

__Nushell example__:

```
dragoon change-available-send-storage --node 127.0.0.1:3001 10000000000
```

It returns:

`"New total storage space is 10000000000, 956 is already used so the remaining available size for send blocks is 9999999044"`

**Note**: This does not work currently due to limitations in Nushell's http post

__cURL example__:

```
curl -X POST "http://127.0.0.1:3001/change-available-send-storage" -H "Content-Type: Application/Json" -d '10000000000'
```

This will work as expected
