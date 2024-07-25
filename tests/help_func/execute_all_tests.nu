for test in (ls tests/*.nu | where type == file) {
    nu ($test | get name)
}