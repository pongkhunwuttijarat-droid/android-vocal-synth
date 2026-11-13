using System.IO;
using OpenUtau.Core.Ustx;
using YamlDotNet.Core;
using YamlDotNet.Core.Events;
using YamlDotNet.Serialization;
using YamlDotNet.Serialization.EventEmitters;
using YamlDotNet.Serialization.NamingConventions;

namespace OpenUtau.Core {
    public class Yaml {
        // Binary-compatibility note (T013): classic community phonemizer DLLs
        // (e.g. EN TETO / Tetomizer) contain `ldsfld IDeserializer
        // OpenUtau.Core.Yaml::DefaultDeserializer` — a static FIELD of the raw
        // YamlDotNet interface type. Exposing these members as properties (or
        // as fields of the facade type) breaks binary binding with
        // MissingFieldException even when the C# source reads identically.
        // Keep the exact field shapes of the plugin era; builders preserve the
        // old configuration (underscore naming, flow style, no aliases).
        public static readonly ISerializer DefaultSerializer;
        public static readonly IDeserializer DefaultDeserializer;

        static Yaml() {
            DefaultSerializer = new SerializerBuilder()
                .WithNamingConvention(UnderscoredNamingConvention.Instance)
                .ConfigureDefaultValuesHandling(DefaultValuesHandling.OmitNull)
                .WithEventEmitter(next => new FlowEmitter(next))
                .DisableAliases()
                .WithQuotingNecessaryStrings()
                .Build();
            DefaultDeserializer = new DeserializerBuilder()
                .WithNamingConvention(UnderscoredNamingConvention.Instance)
                .IgnoreUnmatchedProperties()
                .Build();
        }
    }

    public class FlowEmitter : ChainedEventEmitter {
        public FlowEmitter(IEventEmitter nextEmitter) : base(nextEmitter) { }
        public override void Emit(MappingStartEventInfo eventInfo, IEmitter emitter) {
            if (eventInfo.Source.Type == typeof(PitchPoint) ||
                eventInfo.Source.Type == typeof(UVibrato) ||
                eventInfo.Source.Type == typeof(UExpression)) {
                eventInfo.Style = MappingStyle.Flow;
            }
            base.Emit(eventInfo, emitter);
        }
        public override void Emit(SequenceStartEventInfo eventInfo, IEmitter emitter) {
            base.Emit(eventInfo, emitter);
        }
    }
}
