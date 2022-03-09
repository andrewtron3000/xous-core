#!/usr/bin/python3

"""
# Intro

key2bits.py will transform a binary layout of a key ROM into a set of bits that should be replaced in
a Spartan-7 bitstream to program the desired key into an FPGA.

The implementation will read in a tilegrid.json and a segbits_clbll_l.db file, as generated by
prjxray, as well as a rom.db file generated by betrusted-soc.py that specifies the mapping of ROM LUTs
to SLICE/BELs, and contains the hard-offset of the frame sequence that would contain the ROM array.

This layout data is combined wth a key.bin file which contains an 8kbit array of key material,
organized as 256, 32-bit big endian words.

There are two types of output from this program:

1. An ascii file containing a set of (offset, byte) pairs that specify the patching offset, in bytes, from the
start of the "type2" frames within the bitstream to insert the ROM pattern. This can be paired wth another
utility that applies the patch thus resulting in a .bin file that can be burned into an FPGA.

2. A Rust function which reads in the key data from a "key: [u32; 256]" array, and given an offset
from the start of a "type2" config frame run return either None for no patching, or the byte to replace at
that offset to insert the key into a bitstream.

The first type of output is used to validate that this code works through manual benching. The second
type of output is meant to be copied into the bitstream encryption routine inside Betrusted to accomplish
the key insertion.

By allowing the striping of the key data to be specified in a rom.db file, the bitstream patching algorithm
can be updated in the case that later on the position of the key LUTs need to be changed for better fitting
into the FPGA.

# rom.db format

KEYROM 0 A SLICE_X36Y50 b'5bbb150b97ae3f53'
KEYROM 0 B SLICE_X36Y50 b'cc73e7358f8ddfa4'
KEYROM 0 C SLICE_X36Y50 b'7c250428eb1c34fc'
KEYROM 0 D SLICE_X36Y50 b'cf001d400920ace8'
KEYROM 1 A SLICE_X37Y50 b'590ce26fdddad8ae'
KEYROM 1 B SLICE_X37Y50 b'38b963e9309f90a9'
  |    | |     |        |_____________________  the value stored in the INIT, for reference only
  |    | |     |______________________________  the SLICE location of the ROM LUT
  |    | |____________________________________  mapping of BEL to ROM address, little endian.
  |    |______________________________________  which bit of the 32-bit bus this LUT maps to
  |___________________________________________  root name of the cells

the SLICE maps to one of two sites in a CLBLL_L block; even slices to "X0", and odd slices to "X1" in
the segbits file.


# MAPPING KEY->LUT

The ROM is implemented in hardware as a 256 entry x 32-bit wide ROM. There are 128 ROM LUTs, and each ROM LUT
maps to one bit of the key ROM. So, KEYROM0* is bit 0, KEYROM1* is bit 1, and so forth. The KEYROMs are further
broken down into A-D BEL positions, with "A" position corresponding to bits [63:0], "B" to bits [127:64],
"C" to bits [191:128], and "D" to bits [255:192] (note that this assumes in rom.db that the BEL mappings
are presented in A,B,C,D order; changing this order would change the mapping of bits).
Within each LUT, the INIT value maps the lower six bits of the address, with address 0 corresponding to
the LSB, and address 63 corresponding to the MSB.

Thus the key data as presented in "program order" needs to be tilted on its side and bit-striped into the
key array:

KEYROM0A is bit 0 of the ROM, addresses 63-0,    so INIT LSB is bit 0 at address 0
KEYROM0B is bit 0 of the ROM, addresses 127-64,  so INIT LSB is bit 0 at address 64
KEYROM1C is bit 0 of the ROM, addresses 191-128, so INIT LSB is bit 0 at address 128
KEYROM1D is bit 0 of the ROM, addresses 255-192, so INIT LSB is bit 0 at address 192
...
KEYROM31A is bit 31 of the ROM, addresses 63-0,  so INIT LSB is bit 31 at address 0
...
KEYROM31D is bit 31 of the ROM, addresses 255-192, so INIT LSB is bit 31 at address 192


# segbits + tilegrid format

Xilinx config frames consist of 101, 32-bit words. Config data for a single CLB is striped across
36 frames. Thus, one can think of a group of CLBs as belonging to a rectangular array of bits
where the X-axis describes which CLB, and the Y-axis describes which function:

                         offset -->
frame N+0 (function A):  CLB_A  CLB_B  CLB_C  CLB_D ...
frame N+1 (function B):  CLB_A  CLB_B  CLB_C  CLB_D ...
frame N+2 (function C):  CLB_A  CLB_B  CLB_C  CLB_D ...
  |
  v  index

From the tilegrid.json file:
"CLBLL_L_X24Y50": {
    "bits": {
        "CLB_IO_CLK": {
            "baseaddr": "0x00000C00",    <-- "frame N"
            "frames": 36,                <-- total number of "functions"
            "offset": 0,                 <-- offset, must be less than 101-2 = 99 in this case
            "words": 2                   <-- number of 32-bit words dedicated to each CLB at a given offset, big-endian
        }
    },
    "grid_x": 62,
    "grid_y": 103,
    "pin_functions": {},
    "sites": {
        "SLICE_X36Y50": "SLICEL",        <-- name of SLICE that will correspond to the ROM LUT location
        "SLICE_X37Y50": "SLICEL"
    },
    "type": "CLBLL_L"

Thus, each CLB is described with a "base address" for a given CLB, and an "offset". The "base address"
refers to the address of a given frame, and the "offset" is the index into the frame, where the
striping of the CLB data starts.

The segbits file then locates a bit describing a CLB element based on a "function_bit" notation.
From the segbits file (all others ignored):

CLBLL_L.SLICEL_X0.ALUT.INIT[00] 32_15
CLBLL_L.SLICEL_X0.ALUT.INIT[01] 33_15
CLBLL_L.SLICEL_X0.ALUT.INIT[02] 32_14
  |        |        |        |   |  |_____ bit offset, 63:0, words in big endian format
  |        |        |        |   |________ function_index
  |        |        |        |____________ bit index of the INIT value
  |        |        |_____________________ BEL of the LUT
  |        |______________________________ SLICE offset (even or odd) within the CLB
  |_______________________________________ CLB type

Thus, to resolve the exact bit position of say, SLICE_X36Y50, LUTA, bit 0:

KEYROM0A bit 0 = (baseaddr + offset + function_index as u64)[bit_offset]
               = (0xC00 + 0x0 + 32) read out as u64 big endian, index to bit 15


Note on the big-endian format. The diagram above is more accurately drawn like this, where
each CLB_ is a 32-bit word in a frame:

                         offset -->
frame N+0 (function A):  CLB_AH  CLB_AL  CLB_BH  CLB_BL ...
frame N+1 (function B):  CLB_AH  CLB_AL  CLB_BH  CLB_BL ...
frame N+2 (function C):  CLB_AH  CLB_AL  CLB_BH  CLB_BL ...
  |
  v  index

So a CLB_B would have an "offset" of 2, and bit 63 would be the MSB of CLB_BH; and
CLB_A would have an "offset" of 0, and bit 15 would be in the middle of CLB_AL. The bit
positions are as extracted by "explorebits.py", that is, the 32-bit words themselves are
extracted in big-endian format from the constituent bytes so that the values literally match
the command values as documented in UG470 (Xilinx's public document describing the bitstream
format).

"""

import argparse
import json
import re
import os
from datetime import datetime

def sum_columns(column_dict, stop):
    sum = 0

    for col in sorted(column_dict):
        if int(col) < stop:
            sum += int(column_dict[col]['frame_count'])

    return sum

"""
Decode a frame address to a framestream position.

Frame addresses are the absolute address of a frame within the FPGA. However,
a typical bitstream does not specify frame addresses. Frames are implicitly addressed,
where each frame in the FPGA is visited in a well-defined order. To patch a bitstream,
one needs to translate the frame address to the position in the bitstream.

This relative position in the bitstream is called a "framestream" position in this program.
"""
def address_to_framestream(db, address):
    minor_address = address & 0x7F
    column_address = (address >> 7) & 0x3FF
    row_address = (address >> 17) & 0x1F
    clock_region_code = (address >> 22) & 0x1
    block_type_code = (address >> 23) & 7

    region_met = False
    row_met = False
    type_met = False
    framestream_offset = 0
    for region in range(len(db['global_clock_regions'])):
        if region == 0:
            clock_region = 'top'
        else:
            clock_region = 'bottom'

        if region >= clock_region_code:
            region_met = True
        if type_met:
            break

        for row in range(len(db['global_clock_regions'][clock_region]['rows'])):
            if region_met and (row >= row_address):
                row_met = True

            if type_met:
                break

            for bt in range(3):
                if (bt >= block_type_code) and region_met and row_met:
                    type_met = True

                if bt == 0:
                    block_type = 'CLB_IO_CLK'
                elif bt == 1:
                    block_type = 'BLOCK_RAM'
                else:
                    block_type = 'CFG_CLB'

                try:
                    columns = db['global_clock_regions'][clock_region]['rows'][str(row)]['configuration_buses'][block_type]['configuration_columns']
                except KeyError as e:
                    continue # if the type isn't in the DB, don't throw an error

                if type_met:
                    framestream_offset += sum_columns(columns, column_address)
                    break # we've resolved all the coordinates, done walking the bitstream output
                else:
                    framestream_offset += sum_columns(columns, 1000) # a large number greater than any # of columns

    return framestream_offset + minor_address

def auto_int(x):
    return int(x, 0)

def main():

    parser = argparse.ArgumentParser(description="Bitstream patcher code generator")
    parser.add_argument("-p", "--part", help="Part frame mapping file", default="db/xc7s50csga324-1il.json", type=str)
    parser.add_argument("-t", "--tilegrid", help="tilegrid file", default="db/tilegrid.json", type=str)
    parser.add_argument("-r", "--romdb", help="ROM LUT mapping database", default="../../../../precursors/rom.db", type=str)
    parser.add_argument("-s", "--segbits", help="segbits file", default="db/segbits_clbll_l.db", type=str)
    parser.add_argument("-o", "--output", help="Output file name", default="patch.rs", type=str)
    args = parser.parse_args()

    #-----------  READ IN DATABASES ------------
    slice_db = {}
    with open(args.tilegrid, "r") as f:
        json_file = f.read()
        db = json.loads(json_file)
        for key in db:
            entry = db[key]
            if 'SLICE' in str(entry['sites']):
                for slices in entry['sites']:
                    slice_db[slices] = entry['bits']['CLB_IO_CLK']
    # slice_db is now a lookup for SLICE locations to addresses and offsets

    with open(args.part, "r") as f:
        partfile = f.read()
        part_db = json.loads(partfile)
    # part_db now contains the frame-to-bitstream position database

    segbits = []
    with open(args.segbits, "r") as f:
        for line in f:
            if line.startswith("CLBLL_L.SLICE") and "INIT[" in line:
                segbits += [line]

    segbits_db = {'X0': {'ALUT':[None]*64, 'BLUT':[None]*64, 'CLUT':[None]*64, 'DLUT':[None]*64},
                  'X1': {'ALUT':[None]*64, 'BLUT':[None]*64, 'CLUT':[None]*64, 'DLUT':[None]*64},}
    for line in segbits:
        elements = re.split('[^a-zA-Z0-9]', line.split()[0])
        bitlist = segbits_db[elements[3]][elements[4]]
        bitlist[int(elements[6])] = line.split()[1].split('_')  # insert the function index + bit offset at the respective LUT entry
    # segbits_db is now a lookup of a slice/lut position to a function index + bit offset

    rom_db = [[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],[],]
    with open(args.romdb, "r") as f:
        for line in f:
            items = line.split()
            rom_db[int(items[1])] += [{'bel': items[2] + 'LUT', 'slice': items[3]}]

    #-----------  DERIVE THE PATCHING LIST ------------
    # at this point, we want to derive a list of addresses to patch in the bitstream,
    # each list entry is a 32-entry dictionary, and each entry corresponds to an (address, bit) position in rom.bin
    patchdata = {}
    for keyrom_data_bit in range(32):
        item = rom_db[keyrom_data_bit]
        for lut in range(4):
            slices = item[lut]
            slice = slices['slice']
            bel = slices['bel']
            base_record = slice_db[slice]
            frameaddress = int(base_record['baseaddr'],16)
            frameindex = int(base_record['offset'])
            xy = re.split('[XY]', slice)
            x = int(xy[1])
            if (x % 2) == 0:
                segloc = segbits_db['X0'][bel]
            else:
                segloc = segbits_db['X1'][bel]
            # I now segloc, which is a 64-entry list, one corresponding to each bit of the ROM INIT LUT
            # each entry is a (function index, bit) pair
            if bel == 'ALUT':
                keyrom_addr_offset = 0
            elif bel == 'BLUT':
                keyrom_addr_offset = 64
            elif bel == 'CLUT':
                keyrom_addr_offset = 128
            else:
                keyrom_addr_offset = 192
            for keyrom_addr_lsb in range(64):
                thisbit_frameaddress = frameaddress
                thisbit_frameindex = frameindex
                keyrom_addr = keyrom_addr_offset + keyrom_addr_lsb

                entry = segloc[keyrom_addr_lsb]

                function_offset = int(entry[0])
                bit_offset = int(entry[1])
                # now convert from 64-bit "function" bit position as documented in segbits to a 32-bit "stream" bit position
                # it's a big-endian mapping
                if bit_offset < 32:
                    thisbit_frameindex += 1
                else:
                    bit_offset -= 32

                thisbit_frameaddress += function_offset

                if thisbit_frameaddress in patchdata:
                    frame = patchdata[thisbit_frameaddress]
                else:
                    frame = [None]*101

                if frame[thisbit_frameindex] == None:
                    frame[thisbit_frameindex] = {}

                # store the keyrom address to bit mapping for a given stream address offset
                if bit_offset in frame[thisbit_frameindex]:
                    print("warning: overwriting a patch bit, should not happen!")
                frame[thisbit_frameindex][bit_offset] = [keyrom_addr, keyrom_data_bit]

                patchdata[thisbit_frameaddress] = frame

    #-----------  SORT PATCH LIST AND TRANSLATE ADDRESS TO FRAMESTREAM POSITION ------------
    with open(args.output, "w") as f:
        patchdata_sorted = []
        for key in sorted(patchdata.keys()):
            patchdata_sorted += [[address_to_framestream(part_db, key), patchdata[key]]]

        # compute the length of the words array to patch in a PatchFrame
        # we're assuming all the patch lengths are the same in every frame
        # This doesn't hold if we place the ROM irregularly or use inhomogenous LUTs:
        # this structure could be of a different size for each frame!
        patchlen = 0
        frame = patchdata_sorted[0][1]
        for word in range(101):
            if frame[word] == None:
                continue
            else:
                patchlen += 1

        # test to see if we can emit an optimized version of the code
        is_wellformed = True
        for frame_rec in patchdata_sorted:
            frame = frame_rec[1]
            words_used = [0] * 101
            for word in range(101):
                if frame[word] == None:
                    continue
                words_used[word] = 1
            if words_used[:32] != [1] * 32:
                is_wellformed = False
            if words_used[32:] != [0] * (101-32):
                is_wellformed = False

        if is_wellformed:
            print("Patch list is well formed, generating optimized code")
            generate_optimized(f, patchdata_sorted, patchlen)
            exit(0)
        else:
            print("Patch list is irregular, generating general-case code (is_wellformed: {})".format(is_wellformed))

        f.write("""
//! this file was auto-generated by key2bits.py on {}
//! manual regeneration is needed for the following cases:
//!   - rom.db changes (that is, the KEYROM pcells have been moved to a new location)
//!   - entirely different FPGA target (in which case, the db/ files also need to be regenerated
//!     (this is a long process if you're not targting a prjxray pre-build target))
//!   - bug fixes and enhancements to these routines
""".format(str(datetime.now())))

        f.write("""
pub const PATCH_FRAMES: [u32; {}] = [\n""".format(len(patchdata_sorted)))
        for frame_rec in patchdata_sorted:
            f.write("    0x{:x},\n".format(frame_rec[0]))
        f.write("];\n")

        f.write("""
#[derive(Copy,Clone)]
pub struct PatchBit {{
    adr: u8,
    bit: u8,
}}
pub struct PatchWord {{
    offset: u8,
    bits: [PatchBit; 32],
}}
pub struct PatchFrame {{
    frame: u32,
    words: [PatchWord; {}],
}}

/// patch a frame at a given relative positition and offset in the framestream
/// to insert a key ROM.
///
/// frame is the frame number in the framestream
/// offset is the offset in the frame, from 0-100 (101 words)
/// note that each primitive in a bitstream is a u32
///
/// we are using a Vec for the frame data, even though frames have a well-known length
/// of up to 101 elements. The reason is that typically the number of items to patch
/// in a frame is usually much less than the 101 elements, and so it is wasteful to code
/// for the unused elements statically. That being said, this results in the initialization
/// of the function taking quite a lot of code space, as every element is turned into a
/// load/store pair as the Vecs are dynamically allocated. One possible optimization
/// is to determine the maximum length of any PatchFrame array and create a coding for
/// words that should be skipped in case the frames are not homogenous in patch location
/// vs length, and then turn these into slices or a [T; N] notation const. However,
/// we'll stick with this for now as there are more important thing to do than optimize
/// this code.
///
/// returns None if the position should not be patched
/// returns a tuple of the value and its inverse; this is done to reduce the timing
/// sidechannel. The inverse value needs to be consumed to prevent the compiler
/// from optimizing out that path.
pub fn patch_frame(frame: u32, offset: u32, rom: &[u32]) -> Option<(u32, u32)> {{\n""".format(patchlen))

        for frame_rec in patchdata_sorted:
            frame = frame_rec[1]
            patchvec = ""
            for word in range(101):
                if frame[word] == None:
                    continue
                else:
                    wordvalue = 0
                    wordbits = frame[word]
                    thebits = ""
                    for bit in range(32):
                        coord = wordbits[bit]
                        thebits += "                         PatchBit { adr: " + "{:3}".format(coord[0]) + ", bit: " + "{:2}".format(coord[1]) + " },\n"
                        patchbit =  "{}".format(thebits)
                    patchword = "\n            PatchWord { offset: " + "{}".format(word) +",\n                       bits:\n                      [\n" + "{}".format(patchbit) + "                     ] },"
                    patchvec += patchword
            f.write("   let frame_{:x}".format(frame_rec[0]) + ": PatchFrame = PatchFrame {\n       frame: 0x" + "{:x}".format(frame_rec[0]) + ",\n       words: [" + "{}".format(patchvec) + "] };\n")

        f.write("""
    let table = [\n""")
        for frame_rec in patchdata_sorted:
            f.write("        frame_{:x},\n".format(frame_rec[0]))
        f.write(    """    ];

    for frames in table.iter() {
        if frames.frame == frame {
            for word in frames.words.iter() {
                if word.offset == offset as u8 {
                    let mut data: u32 = 0;
                    let mut data_inv: u32 = 0;
                    for bit in 0..32 {
                        let patch: PatchBit = word.bits[bit];
                        let romval: u32 = rom[patch.adr as usize] & (1 << patch.bit);
                        if romval != 0 {
                            data |= 1 << bit;
                        } else {
                            data_inv |= 1 << bit;
                        }
                    }
                    return Some((data, data_inv))
                }
            }
        }
    }

    return None
}
""")

        frame_nos = []
        for frame_rec in patchdata_sorted:
            frame_nos.append(frame_rec[0])
        f.write("""
/// a fast check to see if a given frame is within the range of frames that are
/// patchable. Use this to wrap calls to `patch_frame` as a performance optimization.
pub fn should_patch(frame: u32) -> bool {{
    if frame >= 0x{:x} && frame <= 0x{:x} {{
        true
    }} else {{
        false
    }}
}}
""".format(min(frame_nos), max(frame_nos)))
        generate_test(f, patchdata_sorted)

def generate_test(f, patchdata_sorted):
        f.write("""
#[cfg(test)]
mod tests {
    #[test]
    fn check_frames() {
        const ROM: [u32; 256] = [\n""")
        # build some random test vectors
        keyrom = []
        for i in range(256):
            keyrom += [int().from_bytes(os.urandom(4), byteorder='big', signed=False)]

        for word in keyrom:
            f.write('               0x{:08x},\n'.format(word))
        f.write("""
        ];\n""")
        for frame_rec in patchdata_sorted:
            frame = frame_rec[1]
            for word in range(101):
                if frame[word] == None:
                    break
                else:
                    wordvalue = 0
                    wordbits = frame[word]
                    for bit in range(32):
                        coord = wordbits[bit]
                        bitvalue = (keyrom[coord[0]] & (1 << coord[1]))
                        if bitvalue != 0:
                            wordvalue |= (1 << bit)
                    f.write('        assert_eq!(crate::key2bits::patch_frame({}'.format(frame_rec[0]) + ', ' + '{}'.format(word) + ', &ROM), Some((' + '0x{:08x}u32'.format(wordvalue) + ', !0x{:08x}u32'.format(wordvalue) + ')));\n')

        f.write('        // also test the null case, frame 0 should typically have no mappings.\n')
        f.write('        assert_eq!(crate::key2bits::patch_frame(0x0, 0, &ROM), None );\n')
        f.write("""
    }
}
""")

def generate_optimized(f, patchdata_sorted, patchlen):
        sequences = []
        sequence = []
        prev_frame = None
        for frame_rec in patchdata_sorted:
            if prev_frame == None:
                prev_frame = frame_rec[0]
                sequence.append(frame_rec)
            else:
                if frame_rec[0] - prev_frame != 1:
                    sequences.append(sequence)
                    sequence = [frame_rec]
                else:
                    sequence.append(frame_rec)
                prev_frame = frame_rec[0]
        sequences.append(sequence)

        # import pprint
        # pprint.pprint(sequences, indent=6)

        f.write("""
//! this file was auto-generated by key2bits.py on {}
//! manual regeneration is needed for the following cases:
//!   - rom.db changes (that is, the KEYROM pcells have been moved to a new location)
//!   - entirely different FPGA target (in which case, the db/ files also need to be regenerated
//!     (this is a long process if you're not targting a prjxray pre-build target))
//!   - bug fixes and enhancements to these routines
""".format(str(datetime.now())))

        f.write("""
pub const PATCH_FRAMES: [u32; {}] = [\n""".format(len(patchdata_sorted)))
        for frame_rec in patchdata_sorted:
            f.write("    0x{:x},\n".format(frame_rec[0]))
        f.write("];\n")

        for sequence in sequences:
            f.write("""
const PATCH_TABLE_{}: [u8; {}] = [\n""".format(sequence[0][0], len(sequence) * 2 * 32 * 32))
            for frame_rec in sequence:
                f.write("    // frame 0x{:x} ({})".format(frame_rec[0], frame_rec[0]))
                for bitdict in frame_rec[1]:
                    if bitdict == None:
                        continue
                    for index in range(32):
                        if (index % 16 == 0):
                            f.write("\n    ")
                        f.write("{:03},{:03},".format(bitdict[index][0], bitdict[index][1]))
                f.write("\n")
            f.write("];\n")

        f.write("""
/// patch a frame at a given relative positition and offset in the framestream
/// to insert a key ROM.
///
/// frame is the frame number in the framestream
/// offset is the offset in the frame, from 0-100 (101 words)
/// note that each primitive in a bitstream is a u32
///
/// This code path has been optimized because the patch set is well-formed, such that
/// the patches are all located in frames 0-31, and are mostly sequential in frames.
/// This allows us to use direct array accessors with index operations, instead of a
/// more general data structure. This method is less readable, but saves a significant
/// amount of code space and reduces compile times. Refer to the generation script
/// if a more general form is desired; this option is retained for reference purposes.
///
/// returns None if the position should not be patched
/// returns a tuple of the value and its inverse; this is done to reduce the timing
/// sidechannel. The inverse value needs to be consumed to prevent the compiler
/// from optimizing out that path.
""")

        f.write("""
/// a fast check to see if a given frame is within the range of frames that are
/// patchable. Use this to wrap calls to `patch_frame` as a performance optimization.
pub fn should_patch(frame: u32) -> bool {
""")
        f.write("    if")
        first_element = True
        for sequence in sequences:
            if first_element:
                f.write(" (frame >= 0x{:x} && frame <= 0x{:x})\n".format(sequence[0][0], sequence[-1][0]))
                first_element = False
            else:
                f.write("       || (frame >= 0x{:x} && frame <= 0x{:x})\n".format(sequence[0][0], sequence[-1][0]))
        f.write("""    {
        true
    } else {
        false
    }
}""")

        f.write("""
fn get_patch_subtable(frame: u32, offset: u32, table_base: u32) -> &'static [u8] {
    // ASSUME: all offsets are checked before here. Worst case, we throw a panic because of an OOB index.
    // A frame table consists of:
    //   - 32 patchable words in a frame (it is a convenient coincidence that the
    //     patchable words are at positions 0-31: if the ROM LUTs were implemented
    //     differently, this may not be the case)
    //   - Each word to patch is 32 bits long, thus requiring as many entries
    //   - Each entry consisting of 2 items:
    //     - A u8 address in the conventionally-ordered source key ROM
    //     - A u8 bit position of the data at that address in the same key ROM
    // Thus a patch table says for a given frame and offset, you may replace the
    // values with those found at the given address and bit offsets in a key ROM
    // to program that key ROM into the LUT
    let base = ((frame - table_base) * 2 * 32 * 32 + (offset * 2 * 32)) as usize;
    match table_base {
""")
        for sequence in sequences:
            f.write("        {} => &PATCH_TABLE_{}[base..base + 2 * 32],\n".format(sequence[0][0], sequence[0][0]))
        f.write("""        _ => panic!("invalid table base in patch.rs")
    }
}
""")

        f.write("""
pub fn patch_frame(frame: u32, offset: u32, rom: &[u32]) -> Option<(u32, u32)> {
    if offset >= 32 {
        return None;
    }
    if should_patch(frame) {
        // ASSUME: `frame` meets the requirements outlined above
        let table_base = match frame {
""")
        for sequence in sequences:
            base = sequence[0][0]
            for run in sequence:
                f.write("            {} => {},\n".format(run[0], base))
        f.write("""            _ => panic!("invalid frame in patch.rs"),
        };
        let subtable = get_patch_subtable(frame, offset, table_base);
        let mut data: u32 = 0;
        let mut data_inv: u32 = 0;
        for bit in 0..32 {
            let (adr, patch_bit) = (subtable[bit * 2], subtable[bit * 2 + 1]);
            let romval: u32 = rom[adr as usize] & (1 << patch_bit);
            if romval != 0 {
                data |= 1 << bit;
            } else {
                data_inv |= 1 << bit;
            }
        }
        Some((data, data_inv))
    } else {
        None
    }
}
""")
        generate_test(f, patchdata_sorted)

if __name__ == "__main__":
    main()
