# Demonstration

A demonstration using an encoded file with $(k,n) = (3,5)$ (meaning $3$ blocks are needed to decode the file and $5$ blocks are produced) is available in [tests/get_file_4_peers_min_blocks.nu](../tests/get_file_4_peers_min_blocks.nu).

The blocks are produced, two are deleted, put on different nodes, and a last node gets the blocks and decodes the file. It then checks the decoded file against the original.
If making the test manually in local (ie without using ssh to connect to different computers), every part using ssh can be ignored for the demonstration.

A second part to the demonstration (which is not done in the test), could consist in the following:
- Clear the blocks on the node 3, as well as the decoded file
- Choose a block you wish to corrupt
- Go to the block location on disk (the path should be `~/.share/dragoonfly/PEER_ID/files/FILE_HASH/blocks/BLOCK_HASH`)
- If using ssh, first ssh on the computer hosting the node process
- Use your favorite text editor (vim, nano, emacs, etc.) and change one or more byte of data on the block
- Now try to use get-file on the node 3 again, it should fail since now one block is corrupted; only 2 correct blocks remain, meaning it's impossible to decode the file
