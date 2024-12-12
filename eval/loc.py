import glob

def walk(path):
    s = 0
    for filename in glob.iglob(path + '**/*.rs', recursive=True):
        with open(filename) as f:
            mlc = False
            for l in f:
                l = l.strip()
                if not l:
                    continue
                
                if mlc:
                    if l.endswith('*/'):
                        mlc = False
                    else:
                        continue
                
                if l.startswith('//'):
                    continue
                if l.startswith('/*'):
                    if not l.endswith('*/'):
                        mlc = True
                    continue
                s += 1
            assert mlc == False

    print(s)

walk("../src/rust/")

# old 22885
# new 27816

# wo comments
# old 18187
# SOSP'24 21808 (3621)
# OSDI'25 22614 (4427)


# prism 18187 (1812)
# prism app 958
# proxy app 670