// Throwaway. Roadmap package X7. Run by the engine's headless analyzer as a post-script.
// Same output format as the Rust dump: F lines for every function, D blocks for the
// functions whose name holds the filter.
//   args: <out-file> <filter>
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import java.io.PrintWriter;
import java.io.FileWriter;

public class GhidraDump extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        String filter = a.length > 1 ? a[1] : "";
        try (PrintWriter out = new PrintWriter(new FileWriter(a[0]))) {
            out.println("L\t" + currentProgram.getLanguageID() + "\timageBase=0x"
                + Long.toHexString(currentProgram.getImageBase().getOffset()));
            DecompInterface di = new DecompInterface();
            di.openProgram(currentProgram);
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
            java.util.List<Function> all = new java.util.ArrayList<>();
            while (it.hasNext()) {
                Function f = it.next();
                all.add(f);
                out.println("F\t0x" + Long.toHexString(f.getEntryPoint().getOffset()) + "\t"
                    + f.getName(true) + "\t" + f.getSymbol().getSource()
                    + (f.isThunk() ? "\tthunk" : "") + (f.isExternal() ? "\texternal" : ""));
            }
            // A filter that starts with `@` names a file of addresses as the port prints them
            // (image base 0), one per line; the engine's image base is added to each.
            java.util.Set<Long> wanted = null;
            if (filter.startsWith("@")) {
                wanted = new java.util.HashSet<>();
                long base = currentProgram.getImageBase().getOffset();
                for (String l : java.nio.file.Files.readAllLines(java.nio.file.Paths.get(filter.substring(1)))) {
                    l = l.trim();
                    if (!l.isEmpty()) wanted.add(base + Long.parseUnsignedLong(l.substring(2), 16));
                }
            }
            for (Function f : all) {
                if (f.isExternal()) continue;
                String n = f.getName(true);
                boolean keep = wanted != null ? wanted.contains(f.getEntryPoint().getOffset())
                    : filter.startsWith("^") ? n.startsWith(filter.substring(1))
                    : (filter.isEmpty() || n.contains(filter));
                if (!keep) continue;
                long t0 = System.currentTimeMillis();
                DecompileResults r = di.decompileFunction(f, 120, monitor);
                String text;
                if (r.getDecompiledFunction() != null) text = r.getDecompiledFunction().getC();
                else text = "//ERROR " + r.getErrorMessage() + "\n";
                int lines = text.split("\n", -1).length - (text.endsWith("\n") ? 1 : 0);
                out.println("D\t0x" + Long.toHexString(f.getEntryPoint().getOffset()) + "\t"
                    + f.getName(true) + "\t" + lines + "\t" + (System.currentTimeMillis() - t0) + "ms");
                out.print(text);
                if (!text.endsWith("\n")) out.println();
                out.println("E");
            }
        }
    }
}
