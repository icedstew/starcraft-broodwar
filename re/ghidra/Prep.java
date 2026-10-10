// -preScript: disable analyzers that thrash on SC:R's junk-byte obfuscation.
import ghidra.app.script.GhidraScript;

public class Prep extends GhidraScript {
    @Override
    public void run() throws Exception {
        setAnalysisOption(currentProgram, "Non-Returning Functions - Discovered", "false");
    }
}
