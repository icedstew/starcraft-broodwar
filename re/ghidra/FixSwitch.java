// Rebuild a function Ghidra split apart and give its jump table explicit targets, then decompile it.
//   analyzeHeadless ... -postScript FixSwitch.java <entry> <end> <jmp_insn> <table> <ncases> <out.c>
// Table entries are 32-bit RVAs (MSVC x64 style).
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;

public class FixSwitch extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address entry = toAddr(Long.parseUnsignedLong(a[0].replace("0x", ""), 16));
        Address end = toAddr(Long.parseUnsignedLong(a[1].replace("0x", ""), 16));
        Address jmp = toAddr(Long.parseUnsignedLong(a[2].replace("0x", ""), 16));
        Address table = toAddr(Long.parseUnsignedLong(a[3].replace("0x", ""), 16));
        int n = Integer.parseInt(a[4]);
        FunctionManager fm = currentProgram.getFunctionManager();
        // Drop fragment functions inside the range.
        java.util.List<Address> drop = new java.util.ArrayList<>();
        fm.getFunctionsOverlapping(new AddressSet(entry, end.subtract(1))).forEachRemaining(g -> {
            if (!g.getEntryPoint().equals(entry)) drop.add(g.getEntryPoint());
        });
        for (Address e : drop) fm.removeFunction(e);
        Function f = fm.getFunctionAt(entry);
        AddressSet body = new AddressSet(entry, end.subtract(1));
        if (f == null) {
            f = createFunction(entry, "iscript_run");
        }
        f.setBody(body);
        long base = currentProgram.getImageBase().getOffset();
        ReferenceManager rm = currentProgram.getReferenceManager();
        for (int i = 0; i < n; i++) {
            long rva = getInt(table.add(4L * i)) & 0xffffffffL;
            Address t = toAddr(base + rva);
            rm.addMemoryReference(jmp, t, RefType.COMPUTED_JUMP, SourceType.USER_DEFINED, 0);
            disassemble(t);
        }
        // Jumps that stay inside the body are branches, not tail calls.
        for (Instruction ins : currentProgram.getListing().getInstructions(body, true)) {
            if (ins.getMnemonicString().equalsIgnoreCase("JMP")) {
                ins.setFlowOverride(ghidra.program.model.listing.FlowOverride.NONE);
                for (Address t : ins.getFlows()) {
                    if (body.contains(t)) ins.setFlowOverride(ghidra.program.model.listing.FlowOverride.BRANCH);
                }
                for (Reference r : ins.getReferencesFrom()) {
                    if (body.contains(r.getToAddress()) && r.getReferenceType().isCall()) {
                        ins.setFlowOverride(ghidra.program.model.listing.FlowOverride.BRANCH);
                    }
                }
            }
        }
        DecompInterface di = new DecompInterface();
        di.openProgram(currentProgram);
        DecompileResults r = di.decompileFunction(f, 600, monitor);
        try (PrintWriter w = new PrintWriter(new FileWriter(a[5]))) {
            w.println("// ==== " + entry + " " + f.getName());
            w.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// failed: " + r.getErrorMessage());
        }
    }
}
