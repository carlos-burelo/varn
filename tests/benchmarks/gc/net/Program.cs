using System.Diagnostics;
class Node { public int id; public Node(int i) { id = i; } }
static class P {
    static long sink;
    static long W1() { long t = 0; for (int i = 0; i < 300; i++) { var a = new long[500000]; Array.Fill(a, (long)i); sink += a[7]; t += a.Length; } return t; }
    static long W3() {
        var keep = new List<Node>(); for (int i = 0; i < 4000000; i++) keep.Add(new Node(i));
        var ring = new Node[1024]; for (int i = 0; i < 1024; i++) ring[i] = new Node(0);
        for (int i = 0; i < 4000000; i++) ring[i & 1023] = new Node(i);
        return keep.Count + ring[5].id;
    }
    static long W5() {
        int size = 500000; var win = new List<Node>(size); for (int i = 0; i < size; i++) win.Add(new Node(i));
        long s = 0; for (int i = 0; i < 6000000; i++) { win[i % size] = new Node(i); s += win[(i + 7) % size].id; }
        return s;
    }
    static void Main(string[] a) {
        var sw = Stopwatch.StartNew();
        long r = a[0] switch { "w1" => W1(), "w3" => W3(), _ => W5() };
        sw.Stop();
        var pause = GC.GetTotalPauseDuration().TotalMilliseconds;
        Console.WriteLine("{\"wall_ms\":" + sw.ElapsedMilliseconds + ",\"gc0\":" + GC.CollectionCount(0) + ",\"gc1\":" + GC.CollectionCount(1) + ",\"gc2\":" + GC.CollectionCount(2) + ",\"pause_total_ms\":" + pause.ToString("F1", System.Globalization.CultureInfo.InvariantCulture) + ",\"result\":" + (r + (sink & 1)) + "}");
    }
}
