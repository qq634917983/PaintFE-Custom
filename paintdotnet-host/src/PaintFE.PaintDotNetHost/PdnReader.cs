using System.Collections;
using System.Formats.Nrbf;
using System.IO.Compression;
using System.Reflection;

static class PdnReader
{
    private const int MaxDimension = 25_000;
    private const int MaxLayers = 256;
    private const long MaxDecodedBytes = 1024L * 1024 * 1024;
    private const int MaxStoredChunkBytes = 64 * 1024 * 1024;

    public static PdnDocument Read(string path)
    {
        using var stream = File.OpenRead(path);
        Span<byte> magic = stackalloc byte[4];
        stream.ReadExactly(magic);
        if (!magic.SequenceEqual("PDN3"u8))
            throw new InvalidDataException("Unsupported PDN version (expected PDN3)");

        Span<byte> headerSizeBytes = stackalloc byte[4];
        stream.ReadExactly(headerSizeBytes[..3]);
        var headerSize = BitConverter.ToInt32(headerSizeBytes);
        if (headerSize is < 0 or > 16 * 1024 * 1024)
            throw new InvalidDataException("Invalid PDN header size");
        stream.Seek(headerSize, SeekOrigin.Current);
        if (stream.ReadByte() != 0 || stream.ReadByte() != 1)
            throw new InvalidDataException("Invalid PDN data marker");

        // NrbfDecoder parses records without instantiating serialized types or running callbacks.
        var root = NrbfDecoder.Decode(stream, leaveOpen: true);
        var width = Number(Raw(root, "width"));
        var height = Number(Raw(root, "height"));
        if (width is <= 0 or > MaxDimension || height is <= 0 or > MaxDimension)
            throw new InvalidDataException($"Invalid PDN canvas size: {width}x{height}");
        var rgbaLength = checked((long)width * height * 4);
        if (rgbaLength > MaxDecodedBytes)
            throw new InvalidDataException("PDN canvas exceeds the decoded-size limit");

        var layerList = Raw(root, "layers");
        var layerCount = Number(Raw(layerList, "ArrayList+_size"));
        if (layerCount is <= 0 or > MaxLayers)
            throw new InvalidDataException($"Invalid PDN layer count: {layerCount}");
        if ((long)layerCount * rgbaLength > int.MaxValue)
            throw new InvalidDataException("PDN project exceeds the decoded-size limit");
        var items = ArrayItems(Raw(layerList, "ArrayList+_items"));
        if (items.Count < layerCount)
            throw new InvalidDataException("PDN layer array is truncated");

        var metadata = new List<PdnLayer>(layerCount);
        var pixels = new MemoryStream(checked((int)((long)layerCount * rgbaLength)));
        for (var index = 0; index < layerCount; index++)
        {
            var bitmapLayer = items[index]
                ?? throw new InvalidDataException($"PDN layer {index} is missing");
            if (Number(Raw(bitmapLayer, "Layer+width")) != width ||
                Number(Raw(bitmapLayer, "Layer+height")) != height)
                throw new InvalidDataException($"PDN layer {index} dimensions do not match the canvas");

            var properties = Raw(bitmapLayer, "Layer+properties", "properties");
            var surface = Raw(bitmapLayer, "surface");
            var stride = Number(Raw(surface, "stride"));
            var memoryBlock = Raw(surface, "scan0");
            var sourceLength = LongNumber(Raw(memoryBlock, "length64"));
            if (stride <= 0 || sourceLength <= 0 || sourceLength > MaxDecodedBytes)
                throw new InvalidDataException($"Invalid pixel storage for PDN layer {index}");

            var rawPixels = ReadSurface(stream, sourceLength);
            WriteRgba(pixels, rawPixels, width, height, stride);
            metadata.Add(new PdnLayer(
                Text(Raw(properties, "name"), $"Layer {index + 1}"),
                Boolean(Raw(properties, "visible"), true),
                ByteNumber(Raw(properties, "opacity"), 255),
                ReadBlendMode(bitmapLayer, properties)));
        }

        return new(width, height, metadata.ToArray(), pixels.ToArray());
    }

    private static byte[] ReadSurface(Stream stream, long length)
    {
        var format = stream.ReadByte();
        if (format is not (0 or 1)) throw new InvalidDataException("Unsupported PDN surface format");
        var chunkSize = checked((int)ReadUInt32BigEndian(stream));
        if (chunkSize <= 0) throw new InvalidDataException("Invalid PDN chunk size");
        var chunkCount = checked((int)((length + chunkSize - 1) / chunkSize));
        var output = new byte[checked((int)length)];
        var found = new bool[chunkCount];

        for (var i = 0; i < chunkCount; i++)
        {
            var number = checked((int)ReadUInt32BigEndian(stream));
            var storedLength = checked((int)ReadUInt32BigEndian(stream));
            if (number < 0 || number >= chunkCount || found[number] || storedLength < 0 ||
                storedLength > MaxStoredChunkBytes ||
                stream.CanSeek && storedLength > stream.Length - stream.Position)
                throw new InvalidDataException("Invalid PDN chunk table");
            found[number] = true;
            var stored = new byte[storedLength];
            stream.ReadExactly(stored);
            var offset = checked(number * chunkSize);
            var expected = (int)Math.Min(chunkSize, length - offset);
            if (format == 0)
            {
                using var gzip = new GZipStream(new MemoryStream(stored, writable: false), CompressionMode.Decompress);
                gzip.ReadExactly(output.AsSpan(offset, expected));
                if (gzip.ReadByte() != -1) throw new InvalidDataException("PDN chunk expands beyond its declared size");
            }
            else
            {
                if (storedLength != expected) throw new InvalidDataException("Invalid uncompressed PDN chunk size");
                stored.CopyTo(output, offset);
            }
        }
        return output;
    }

    private static void WriteRgba(Stream destination, byte[] source, int width, int height, int stride)
    {
        var bpp = checked(stride / width);
        if (bpp is not (3 or 4) || stride < width * bpp || source.LongLength < (long)stride * height)
            throw new InvalidDataException("Only 24-bit and 32-bit PDN bitmap layers are supported");
        Span<byte> rgba = stackalloc byte[4];
        for (var y = 0; y < height; y++)
        for (var x = 0; x < width; x++)
        {
            var offset = checked(y * stride + x * bpp);
            rgba[0] = source[offset + 2];
            rgba[1] = source[offset + 1];
            rgba[2] = source[offset];
            rgba[3] = bpp == 4 ? source[offset + 3] : (byte)255;
            destination.Write(rgba);
        }
    }

    private static string ReadBlendMode(object bitmapLayer, object properties)
    {
        var blendMode = TryRaw(properties, "blendMode");
        if (blendMode is not null)
            return Number(TryRaw(blendMode, "value__")) switch
            {
                1 => "Multiply", 2 => "Additive", 3 => "ColorBurn", 4 => "ColorDodge",
                5 => "Reflect", 6 => "Glow", 7 => "Overlay", 8 => "Difference",
                9 => "Negation", 10 => "Lighten", 11 => "Darken", 12 => "Screen",
                13 => "Xor", _ => "Normal"
            };
        var oldProperties = TryRaw(bitmapLayer, "properties");
        var blendOp = oldProperties is null ? null : TryRaw(oldProperties, "blendOp");
        var typeName = blendOp is null ? "" : RecordTypeName(blendOp);
        foreach (var name in new[] { "Multiply", "Additive", "ColorBurn", "ColorDodge", "Reflect", "Glow",
                     "Overlay", "Difference", "Negation", "Lighten", "Darken", "Screen", "Xor" })
            if (typeName.Contains(name, StringComparison.OrdinalIgnoreCase)) return name;
        return "Normal";
    }

    private static object Raw(object record, params string[] names)
    {
        foreach (var name in names)
            if (TryRaw(record, name) is { } value) return value;
        throw new InvalidDataException($"Missing PDN field: {string.Join(" or ", names)}");
    }

    private static object? TryRaw(object record, string name)
    {
        var method = record.GetType().GetMethod("GetRawValue", BindingFlags.Instance | BindingFlags.Public);
        if (method is null) return null;
        try { return method.Invoke(record, [name]); }
        catch (TargetInvocationException) { return null; }
    }

    private static List<object?> ArrayItems(object record)
    {
        if (record is IEnumerable enumerable) return enumerable.Cast<object?>().ToList();
        foreach (var method in record.GetType().GetMethods(BindingFlags.Instance | BindingFlags.Public)
                     .Where(candidate => candidate.Name == "GetArray"))
        {
            var parameters = method.GetParameters();
            object? result = null;
            if (parameters.Length == 1 && parameters[0].ParameterType == typeof(Type))
                result = method.Invoke(record, [typeof(object[])]);
            else if (parameters.Length == 1 && parameters[0].ParameterType == typeof(bool))
                result = method.Invoke(record, [true]);
            else if (parameters.Length == 2 && parameters[0].ParameterType == typeof(Type))
                result = method.Invoke(record, [typeof(object[]), true]);
            if (result is IEnumerable values) return values.Cast<object?>().ToList();
        }
        throw new InvalidDataException("Unsupported PDN array record");
    }

    private static string RecordTypeName(object record) =>
        record.GetType().GetProperty("TypeName")?.GetValue(record)?.ToString() ?? record.GetType().Name;
    private static int Number(object? value) => Convert.ToInt32(value);
    private static long LongNumber(object? value) => Convert.ToInt64(value);
    private static byte ByteNumber(object? value, byte fallback) => value is null ? fallback : Convert.ToByte(value);
    private static bool Boolean(object? value, bool fallback) => value is null ? fallback : Convert.ToBoolean(value);
    private static string Text(object? value, string fallback) => value?.ToString() ?? fallback;
    private static uint ReadUInt32BigEndian(Stream stream)
    {
        Span<byte> bytes = stackalloc byte[4];
        stream.ReadExactly(bytes);
        return System.Buffers.Binary.BinaryPrimitives.ReadUInt32BigEndian(bytes);
    }
}

sealed record PdnDocument(int Width, int Height, PdnLayer[] Layers, byte[] Pixels);
sealed record PdnLayer(string Name, bool Visible, byte Opacity, string BlendMode);
