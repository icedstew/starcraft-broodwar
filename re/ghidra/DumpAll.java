// Decompile every function (in parallel, all cores) to one C file:
//   analyzeHeadless ... -postScript DumpAll.java <out.c>
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.app.decompiler.parallel.*;
import ghidra.program.model.listing.*;
import ghidra.util.task.TaskMonitor;
import java.io.*;
import java.util.*;

public class DumpAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String out = getScriptArgs()[0];
        List<Function> funcs = new ArrayList<>();
        currentProgram.getFunctionManager().getFunctions(true).forEach(funcs::add);
        println("decompiling " + funcs.size() + " functions");

        DecompilerCallback<String[]> cb = new DecompilerCallback<>(currentProgram, di -> {
            di.toggleCCode(true);
            di.toggleSyntaxTree(false);
            di.setSimplificationStyle("decompile");
        }) {
            @Override
            public String[] process(DecompileResults r, TaskMonitor m) {
                Function f = r.getFunction();
                String body = r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// decompile failed: " + r.getErrorMessage();
                return new String[] { f.getEntryPoint().toString(), "// ==== " + f.getEntryPoint() + " " + f.getName() + "\n" + body };
            }
        };
        cb.setTimeout(60);
        List<String[]> results;
        try {
            results = ParallelDecompiler.decompileFunctions(cb, funcs, monitor);
        } finally {
            cb.dispose();
        }
        results.sort(Comparator.comparing(a -> a[0]));
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(out)))) {
            for (String[] r : results) {
                w.println(r[1]);
            }
        }
        println("decompiled " + results.size() + " functions -> " + out);
    }
}
