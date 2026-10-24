// Decompile functions in parallel, appending to <out.c> in batches:
//   analyzeHeadless <proj> sc -process -noanalysis -postScript DumpAll.java <out.c> [addresses.txt]
// addresses.txt: one hex entry per line, in the order to decompile (default: all functions).
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.app.decompiler.parallel.*;
import ghidra.program.model.listing.*;
import ghidra.util.task.TaskMonitor;
import java.io.*;
import java.nio.file.*;
import java.util.*;

public class DumpAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String out = args[0];
        FunctionManager fm = currentProgram.getFunctionManager();
        Set<Function> funcs = new LinkedHashSet<>();
        if (args.length > 1 && args[1].startsWith("except:")) {
            // Every Ghidra function except entries listed in the file, sharded i/n by entry order.
            Set<Long> skip = new HashSet<>();
            for (String line : Files.readAllLines(Paths.get(args[1].substring(7)))) {
                if (!line.trim().isEmpty()) skip.add(Long.parseUnsignedLong(line.trim().replace("0x", ""), 16));
            }
            int shard = Integer.parseInt(args[2]), nshards = Integer.parseInt(args[3]), k = 0;
            for (Function f : fm.getFunctions(true)) {
                if (skip.contains(f.getEntryPoint().getOffset())) continue;
                if (k++ % nshards == shard) funcs.add(f);
            }
        } else if (args.length > 1) {
            for (String line : Files.readAllLines(Paths.get(args[1]))) {
                line = line.trim();
                if (line.isEmpty()) continue;
                ghidra.program.model.address.Address a = toAddr(Long.parseUnsignedLong(line.replace("0x", ""), 16));
                Function f = fm.getFunctionAt(a);
                // .pdata entry with no Ghidra function (obfuscation broke flow): create it (not saved with -readOnly).
                if (f == null) {
                    f = fm.getFunctionContaining(a);
                    if (f == null) {
                        f = createFunction(a, null);
                    }
                }
                if (f != null) funcs.add(f);
            }
        } else {
            fm.getFunctions(true).forEach(funcs::add);
        }
        println("decompiling " + funcs.size() + " functions");

        DecompilerCallback<String> cb = new DecompilerCallback<>(currentProgram, di -> {
            di.toggleCCode(true);
            di.toggleSyntaxTree(false);
            di.setSimplificationStyle("decompile");
        }) {
            @Override
            public String process(DecompileResults r, TaskMonitor m) {
                Function f = r.getFunction();
                String body = r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// decompile failed: " + r.getErrorMessage();
                return "// ==== " + f.getEntryPoint() + " " + f.getName() + "\n" + body;
            }
        };
        String to = System.getenv("DUMP_TIMEOUT");
        cb.setTimeout(to == null ? 30 : Integer.parseInt(to));
        int batch = 500, done = 0;
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(out, true)))) {
            List<Function> all = new ArrayList<>(funcs);
            for (int i = 0; i < all.size() && !monitor.isCancelled(); i += batch) {
                List<Function> chunk = all.subList(i, Math.min(all.size(), i + batch));
                // Results come back in completion order; sort by the fixed-width address header
                // so repeated dumps are identical.
                List<String> res = new ArrayList<>(ParallelDecompiler.decompileFunctions(cb, chunk, monitor));
                Collections.sort(res);
                for (String s : res) {
                    w.println(s);
                }
                w.flush();
                done += chunk.size();
                println("decompiled " + done + "/" + funcs.size());
            }
        } finally {
            cb.dispose();
        }
    }
}
