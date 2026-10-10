// Decompile listed functions one at a time (low memory, long timeout):
//   analyzeHeadless ... -postScript DumpSeq.java <out.c> <addresses.txt> [timeout_s]
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import java.io.*;
import java.nio.file.*;

public class DumpSeq extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        int timeout = a.length > 2 ? Integer.parseInt(a[2]) : 900;
        DecompInterface di = new DecompInterface();
        di.toggleCCode(true);
        di.openProgram(currentProgram);
        FunctionManager fm = currentProgram.getFunctionManager();
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(a[0], true)))) {
            for (String line : Files.readAllLines(Paths.get(a[1]))) {
                line = line.trim();
                if (line.isEmpty()) continue;
                Function f = fm.getFunctionContaining(toAddr(Long.parseUnsignedLong(line.replace("0x", ""), 16)));
                if (f == null) continue;
                DecompileResults r = di.decompileFunction(f, timeout, monitor);
                w.println("// ==== " + f.getEntryPoint() + " " + f.getName());
                w.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// decompile failed: " + r.getErrorMessage());
                w.flush();
                println("done " + f.getEntryPoint());
            }
        }
        di.dispose();
    }
}
